#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod singleton;

use std::collections::VecDeque;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State, WindowEvent};

use tofv_core::{
    appindicator_available, disconnect as kill_session, doctor_report, plan_session,
    probe_live_tunnel, spawn_connect, validate_totp, validate_trusted_cert, AppPaths,
    ConnectOutcome, ConnectRequest, DoctorReport, Elevate, PasswordStore, Profile, RunningConnect,
    SecretToolStore, DEFAULT_PROFILE_ID,
};

const MAX_LOGS: usize = 400;
const TRAY_ICON_PNG: &[u8] = include_bytes!("../icons/32x32.png");
const WINDOW_ICON_PNG: &[u8] = include_bytes!("../icons/128x128.png");
const AUTH_RETRY_MSG: &str =
    "Code refusé. Le FortiToken F121 change toutes les 60 s — saisis le code affiché maintenant.";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum UiStatus {
    Idle,
    Connecting,
    Up,
    Disconnecting,
    NeedCert,
    AuthFailed,
    Error,
}

struct AppState {
    paths: AppPaths,
    status: Mutex<UiStatus>,
    logs: Mutex<VecDeque<String>>,
    last_error: Mutex<Option<String>>,
    need_cert: Mutex<Option<String>>,
    command: Mutex<Option<String>>,
    config_redacted: Mutex<Option<String>>,
    running: Mutex<Option<RunningConnect>>,
    pending_otp: Mutex<Option<String>>,
    cert_probe: Mutex<bool>,
}

impl AppState {
    fn new() -> Result<Self, String> {
        let paths = AppPaths::discover().map_err(|e| e.to_string())?;
        paths.ensure().map_err(|e| e.to_string())?;
        Ok(Self {
            paths,
            status: Mutex::new(UiStatus::Idle),
            logs: Mutex::new(VecDeque::new()),
            last_error: Mutex::new(None),
            need_cert: Mutex::new(None),
            command: Mutex::new(None),
            config_redacted: Mutex::new(None),
            running: Mutex::new(None),
            pending_otp: Mutex::new(None),
            cert_probe: Mutex::new(false),
        })
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProfileView {
    host: String,
    port: u16,
    username: String,
    realm: String,
    trusted_cert: Option<String>,
    auth_method: String,
    has_password: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProfilePatch {
    host: String,
    port: u16,
    username: String,
    realm: String,
    trusted_cert: Option<String>,
}



#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UiSnapshot {
    status: UiStatus,
    profile: ProfileView,
    logs: Vec<String>,
    command: Option<String>,
    config_redacted: Option<String>,
    need_cert: Option<String>,
    last_error: Option<String>,
    doctor: DoctorReport,
}

fn load_profile(paths: &AppPaths) -> tofv_core::Result<Profile> {
    paths.load_default_profile()
}

fn profile_view(paths: &AppPaths) -> Result<ProfileView, String> {
    let p = load_profile(paths).map_err(|e| e.to_string())?;
    let has_password = SecretToolStore
        .get(DEFAULT_PROFILE_ID)
        .map(|v| v.is_some())
        .unwrap_or(false);
    Ok(ProfileView {
        host: p.host,
        port: p.port,
        username: p.username,
        realm: p.realm,
        trusted_cert: p.trusted_cert,
        auth_method: "totp-manual".into(),
        has_password,
    })
}

fn doctor_view() -> DoctorReport {
    doctor_report()
}

fn emit_status(app: &tauri::AppHandle, state: &AppState, status: UiStatus) {
    if let Ok(mut slot) = state.status.lock() {
        *slot = status;
    }
    let _ = app.emit("tofv://status", status);
    if let Some(tray) = app.tray_by_id("main") {
        let label = match status {
            UiStatus::Idle => "TOFV — déconnecté",
            UiStatus::Connecting => "TOFV — connexion…",
            UiStatus::Disconnecting => "TOFV — coupure…",
            UiStatus::Up => "TOFV — connecté",
            UiStatus::NeedCert => "TOFV — certificat",
            UiStatus::AuthFailed => "TOFV — auth refusée",
            UiStatus::Error => "TOFV — erreur",
        };
        let _ = tray.set_tooltip(Some(label));
    }
    if status == UiStatus::Up {
        hide_otp(app);
    }
}

fn push_log(app: &tauri::AppHandle, state: &AppState, line: String) {
    if let Ok(mut logs) = state.logs.lock() {
        if logs.len() >= MAX_LOGS {
            logs.pop_front();
        }
        logs.push_back(line.clone());
    }
    let _ = app.emit("tofv://log", line);
}

fn adopt_live_tunnel(app: &tauri::AppHandle, state: &AppState) -> bool {
    let Some(live) = probe_live_tunnel() else {
        return false;
    };
    emit_status(app, state, UiStatus::Up);
    let iface = live.iface.as_deref().unwrap_or("ppp?");
    push_log(
        app,
        state,
        format!(
            "tofv: tunnel encore actif (pid {}, {iface}) — l'UI précédente a quitté sans Couper. Utilise Couper pour le fermer.",
            live.pid
        ),
    );
    true
}

fn tear_down_vpn(state: &AppState) {
    if let Ok(mut slot) = state.running.lock() {
        if let Some(mut running) = slot.take() {
            let _ = running.terminate();
            drop(running);
        }
    }
    let _ = kill_session(&state.paths);
}

fn show_panel(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn hide_otp(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("otp") {
        let _ = w.hide();
    }
}

const OTP_WIN_W: f64 = 440.0;
const OTP_WIN_H: f64 = 420.0;

fn show_otp_prompt(app: &tauri::AppHandle, retry_msg: Option<&str>) {
    if let Some(w) = app.get_webview_window("otp") {
        let _ = w.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
            OTP_WIN_W, OTP_WIN_H,
        )));
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = app.emit("tofv://ask-otp", retry_msg.unwrap_or(""));
        return;
    }
    match tauri::WebviewWindowBuilder::new(app, "otp", tauri::WebviewUrl::App("otp.html".into()))
        .title("TOFV — FortiToken")
        .inner_size(OTP_WIN_W, OTP_WIN_H)
        .min_inner_size(400.0, 380.0)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(true)
        .center()
        .build()
    {
        Ok(w) => {
            if let Ok(icon) = Image::from_bytes(WINDOW_ICON_PNG) {
                let _ = w.set_icon(icon);
            }
        }
        Err(e) => {
            show_panel(app);
            let _ = app.emit("tofv://ask-otp-fallback", retry_msg.unwrap_or(""));
            let state = app.state::<AppState>();
            push_log(app, &state, format!("tofv: fenêtre TOTP: {e} — modal du panneau"));
        }
    }
}

fn begin_connect_inner(app: &tauri::AppHandle, state: &AppState) -> Result<(), String> {
    let status = *state.status.lock().map_err(|e| e.to_string())?;
    if matches!(
        status,
        UiStatus::Up | UiStatus::Connecting | UiStatus::Disconnecting
    ) {
        return Ok(());
    }
    if let Some(live) = probe_live_tunnel() {
        emit_status(app, state, UiStatus::Up);
        return Err(format!(
            "un tunnel TOFV tourne déjà (pid {}) — Couper d'abord",
            live.pid
        ));
    }
    let doctor = doctor_report();
    if doctor.blocking {
        show_panel(app);
        let _ = app.emit(
            "tofv://toast",
            "Prérequis manquants — vois l’écran d’install.",
        );
        return Ok(());
    }
    match load_profile(&state.paths) {
        Ok(p) if p.validate_ready().is_ok() => {}
        _ => {
            show_panel(app);
            let _ = app.emit(
                "tofv://toast",
                "Complète le profil (hôte, utilisateur) puis Enregistrer.",
            );
            return Ok(());
        }
    }
    let has_password = SecretToolStore
        .get(DEFAULT_PROFILE_ID)
        .map(|v| v.is_some())
        .unwrap_or(false);
    if !has_password {
        show_panel(app);
        let _ = app.emit("tofv://need-password", ());
        return Ok(());
    }
    show_otp_prompt(app, None);
    Ok(())
}

#[tauri::command]
fn begin_connect(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    begin_connect_inner(&app, &state)
}

#[tauri::command]
fn get_state(state: State<'_, AppState>) -> Result<UiSnapshot, String> {
    let profile = profile_view(&state.paths)?;
    let logs = state
        .logs
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .cloned()
        .collect();
    Ok(UiSnapshot {
        status: *state.status.lock().map_err(|e| e.to_string())?,
        profile,
        logs,
        command: state.command.lock().map_err(|e| e.to_string())?.clone(),
        config_redacted: state
            .config_redacted
            .lock()
            .map_err(|e| e.to_string())?
            .clone(),
        need_cert: state.need_cert.lock().map_err(|e| e.to_string())?.clone(),
        last_error: state.last_error.lock().map_err(|e| e.to_string())?.clone(),
        doctor: doctor_view(),
    })
}

#[tauri::command]
fn save_profile(state: State<'_, AppState>, patch: ProfilePatch) -> Result<ProfileView, String> {
    let mut profile = load_profile(&state.paths).map_err(|e| e.to_string())?;
    profile.id = DEFAULT_PROFILE_ID.to_string();
    profile.host = patch.host;
    profile.port = patch.port;
    profile.username = patch.username;
    profile.realm = patch.realm;
    if let Some(cert) = patch.trusted_cert.filter(|s| !s.trim().is_empty()) {
        profile.set_trusted_cert(&cert).map_err(|e| e.to_string())?;
    } else {
        profile.trusted_cert = None;
    }
    profile.auth_method = tofv_core::AuthMethod::TotpManual;
    state
        .paths
        .save_profile(&profile)
        .map_err(|e| e.to_string())?;
    profile_view(&state.paths)
}

#[tauri::command]
fn save_password(password: String) -> Result<(), String> {
    if password.is_empty() {
        return Err("mot de passe vide".into());
    }
    SecretToolStore
        .set(DEFAULT_PROFILE_ID, &password)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn clear_password() -> Result<(), String> {
    SecretToolStore
        .delete(DEFAULT_PROFILE_ID)
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn preview(state: State<'_, AppState>) -> Result<UiSnapshot, String> {
    refresh_preview(&state, "000000")?;
    get_state(state)
}

fn refresh_preview(state: &AppState, otp: &str) -> Result<(), String> {
    let profile = load_profile(&state.paths).map_err(|e| e.to_string())?;
    if profile.validate_ready().is_err() {
        return Ok(());
    }
    let cfg = state.paths.load_app_config().unwrap_or_default();
    let planned = plan_session(ConnectRequest {
        profile: &profile,
        otp,
        paths: &state.paths,
        app_config: &cfg,
        elevate: cfg.elevate,
    })
    .map_err(|e| e.to_string())?;
    *state.command.lock().map_err(|e| e.to_string())? = Some(planned.display);
    *state.config_redacted.lock().map_err(|e| e.to_string())? = Some(planned.config_redacted);
    Ok(())
}

#[tauri::command]
fn connect(app: tauri::AppHandle, state: State<'_, AppState>, otp: String) -> Result<(), String> {
    {
        let running = state.running.lock().map_err(|e| e.to_string())?;
        if running.is_some() {
            return Err("une session est déjà en cours".into());
        }
    }
    if let Some(live) = probe_live_tunnel() {
        emit_status(&app, &state, UiStatus::Up);
        return Err(format!(
            "un tunnel TOFV tourne déjà (pid {}) — Couper d'abord",
            live.pid
        ));
    }

    let otp = otp.trim().to_string();
    validate_totp(&otp).map_err(|_| "le TOTP doit contenir 6 chiffres".to_string())?;
    push_log(
        &app,
        &state,
        "tofv: TOTP saisi — lecture du trousseau, puis pkexec…".into(),
    );

    emit_status(&app, &state, UiStatus::Connecting);
    *state.last_error.lock().map_err(|e| e.to_string())? = None;
    *state.need_cert.lock().map_err(|e| e.to_string())? = None;
    *state.cert_probe.lock().map_err(|e| e.to_string())? = false;
    *state.pending_otp.lock().map_err(|e| e.to_string())? = Some(otp.clone());

    let app = app.clone();
    thread::spawn(move || {
        let state = app.state::<AppState>();
        if let Err(e) = start_session(&app, &state, otp, false) {
            if let Ok(mut err) = state.last_error.lock() {
                *err = Some(e.clone());
            }
            emit_status(&app, &state, UiStatus::Error);
            push_log(&app, &state, format!("tofv: {e}"));
        }
    });
    Ok(())
}

fn start_session(
    app: &tauri::AppHandle,
    state: &AppState,
    otp: String,
    skip_trusted_cert: bool,
) -> Result<(), String> {
    let password = SecretToolStore
        .get(DEFAULT_PROFILE_ID)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "aucun mot de passe dans le trousseau — enregistre-le avec le bouton Trousseau"
                .to_string()
        })?;
    push_log(
        app,
        state,
        "tofv: mot de passe lu, lancement d'openfortivpn…".into(),
    );

    let mut profile = load_profile(&state.paths).map_err(|e| e.to_string())?;
    profile.validate_ready().map_err(|e| e.to_string())?;
    if skip_trusted_cert {
        push_log(
            app,
            state,
            "tofv: essai sans trusted-cert (rotation / découverte de l'empreinte)…".into(),
        );
        profile.trusted_cert = None;
    }
    let cfg = state.paths.load_app_config().unwrap_or_default();
    let elevate = match cfg.elevate {
        Elevate::None => Elevate::Pkexec,
        other => other,
    };
    let _ = refresh_preview(state, &otp);
    let req = ConnectRequest {
        profile: &profile,
        otp: &otp,
        paths: &state.paths,
        app_config: &cfg,
        elevate,
    };
    push_log(
        app,
        state,
        if tofv_core::resolve_helper().is_some() {
            "tofv: démarrage via tofv-helper…".into()
        } else {
            "tofv: helper absent — refuse d’élever openfortivpn (./scripts/install.sh)".into()
        },
    );
    let (running, logs) = spawn_connect(req, password).map_err(|e| e.to_string())?;
    {
        let mut slot = state.running.lock().map_err(|e| e.to_string())?;
        *slot = Some(running);
    }
    let app_logs = app.clone();
    thread::spawn(move || {
        for line in logs {
            let state = app_logs.state::<AppState>();
            push_log(&app_logs, &state, line);
        }
    });
    Ok(())
}

#[tauri::command]
fn disconnect_cmd(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let running = {
        let mut slot = state.running.lock().map_err(|e| e.to_string())?;
        slot.take()
    };
    emit_status(&app, &state, UiStatus::Disconnecting);
    push_log(
        &app,
        &state,
        if tofv_core::resolve_helper().is_some() {
            "tofv: coupure via tofv-helper…".into()
        } else {
            "tofv: coupure sans helper — kill user seulement (un tunnel root restera)".into()
        },
    );
    let app = app.clone();
    thread::spawn(move || {
        let state = app.state::<AppState>();
        if let Some(mut running) = running {
            if let Err(e) = running.terminate() {
                push_log(&app, &state, format!("tofv: coupure: {e}"));
            }
            drop(running);
        }
        let _ = kill_session(&state.paths);
        emit_status(&app, &state, UiStatus::Idle);
        push_log(&app, &state, "tofv: session coupée".into());
    });
    Ok(())
}

#[tauri::command]
fn trust_cert(state: State<'_, AppState>, cert: String) -> Result<ProfileView, String> {
    validate_trusted_cert(&cert).map_err(|e| e.to_string())?;
    let mut profile = load_profile(&state.paths).map_err(|e| e.to_string())?;
    profile.set_trusted_cert(&cert).map_err(|e| e.to_string())?;
    state
        .paths
        .save_profile(&profile)
        .map_err(|e| e.to_string())?;
    if let Ok(mut n) = state.need_cert.lock() {
        *n = None;
    }
    profile_view(&state.paths)
}

#[tauri::command]
fn open_panel(app: tauri::AppHandle) {
    show_panel(&app);
}

#[tauri::command]
fn open_journal(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("journal") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return Ok(());
    }
    let w = tauri::WebviewWindowBuilder::new(
        &app,
        "journal",
        tauri::WebviewUrl::App("log.html".into()),
    )
    .title("TOFV — journal")
    .inner_size(720.0, 520.0)
    .min_inner_size(420.0, 280.0)
    .decorations(false)
    .resizable(true)
    .visible(true)
    .build()
    .map_err(|e| e.to_string())?;
    if let Ok(icon) = Image::from_bytes(WINDOW_ICON_PNG) {
        let _ = w.set_icon(icon);
    }
    Ok(())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NeedCertPayload {
    sha256: String,
    previous: Option<String>,
}

fn apply_outcome(app: &tauri::AppHandle, state: &AppState, outcome: ConnectOutcome) {
    match outcome {
        ConnectOutcome::NeedCert { sha256 } => {
            let _ = state.pending_otp.lock().map(|mut o| o.take());
            if let Ok(mut n) = state.need_cert.lock() {
                *n = Some(sha256.clone());
            }
            let previous = load_profile(&state.paths)
                .ok()
                .and_then(|p| p.trusted_cert)
                .filter(|old| old != &sha256);
            emit_status(app, state, UiStatus::NeedCert);
            hide_otp(app);
            let _ = app.emit(
                "tofv://need-cert",
                NeedCertPayload {
                    sha256: sha256.clone(),
                    previous: previous.clone(),
                },
            );
            show_panel(app);
            if let Some(old) = previous {
                push_log(
                    app,
                    state,
                    format!("tofv: rotation certificat\n  ancien {old}\n  nouveau {sha256}"),
                );
            } else {
                push_log(
                    app,
                    state,
                    format!("tofv: certificat inconnu {sha256}"),
                );
            }
        }
        ConnectOutcome::CertRejected => {
            let probed = state.cert_probe.lock().ok().is_some_and(|g| *g);
            let otp = state
                .pending_otp
                .lock()
                .ok()
                .and_then(|g| g.clone());
            let had_pin = load_profile(&state.paths)
                .ok()
                .and_then(|p| p.trusted_cert)
                .is_some();
            if !probed && had_pin {
                if let Ok(mut g) = state.cert_probe.lock() {
                    *g = true;
                }
                if let Some(otp) = otp {
                    push_log(
                        app,
                        state,
                        "tofv: pin actuel refusé, nouvel essai sans trusted-cert…".into(),
                    );
                    emit_status(app, state, UiStatus::Connecting);
                    let app = app.clone();
                    thread::spawn(move || {
                        let state = app.state::<AppState>();
                        if let Err(e) = start_session(&app, &state, otp, true) {
                            emit_status(&app, &state, UiStatus::Error);
                            push_log(&app, &state, format!("tofv: {e}"));
                        }
                    });
                    return;
                }
            }
            let _ = state.pending_otp.lock().map(|mut o| o.take());
            emit_status(app, state, UiStatus::Error);
            push_log(
                app,
                state,
                "tofv: certificat refusé et pas d'empreinte SHA dans les logs".into(),
            );
        }
        ConnectOutcome::AuthFailed => {
            let _ = state.pending_otp.lock().map(|mut o| o.take());
            if let Ok(mut err) = state.last_error.lock() {
                *err = Some(AUTH_RETRY_MSG.into());
            }
            emit_status(app, state, UiStatus::AuthFailed);
            push_log(
                app,
                state,
                "tofv: authentification refusée — nouveau code FortiToken (fenêtre ~60 s)".into(),
            );
            show_otp_prompt(app, Some(AUTH_RETRY_MSG));
        }
        ConnectOutcome::ExitedAfterUp { code } => {
            let _ = state.pending_otp.lock().map(|mut o| o.take());
            emit_status(app, state, UiStatus::Idle);
            push_log(app, state, format!("tofv: session terminée ({code:?})"));
        }
        ConnectOutcome::Interrupted => {
            let _ = state.pending_otp.lock().map(|mut o| o.take());
            emit_status(app, state, UiStatus::Idle);
        }
        ConnectOutcome::Failed { code } => {
            let _ = state.pending_otp.lock().map(|mut o| o.take());
            emit_status(app, state, UiStatus::Error);
            if let Ok(mut err) = state.last_error.lock() {
                *err = Some(format!("openfortivpn a quitté ({code:?})"));
            }
        }
    }
}

fn poller(app: tauri::AppHandle) {
    loop {
        thread::sleep(Duration::from_millis(200));
        let state = app.state::<AppState>();
        let mut slot = match state.running.lock() {
            Ok(g) => g,
            Err(_) => continue,
        };
        if let Some(running) = slot.as_mut() {
            let snap = running.snapshot();
            if snap.up {
                let current = state.status.lock().ok().map(|s| *s);
                if current != Some(UiStatus::Up) {
                    emit_status(&app, &state, UiStatus::Up);
                }
            }
            match running.try_wait() {
                Ok(Some(outcome)) => {
                    drop(slot);
                    if let Ok(mut g) = state.running.lock() {
                        g.take();
                    }
                    apply_outcome(&app, &state, outcome);
                }
                Ok(None) => {}
                Err(e) => {
                    drop(slot);
                    if let Ok(mut g) = state.running.lock() {
                        g.take();
                    }
                    emit_status(&app, &state, UiStatus::Error);
                    push_log(&app, &state, format!("tofv: {e}"));
                }
            }
            continue;
        }
        drop(slot);

        let current = state.status.lock().ok().map(|s| *s);
        if matches!(
            current,
            Some(
                UiStatus::Connecting
                    | UiStatus::Disconnecting
                    | UiStatus::NeedCert
                    | UiStatus::AuthFailed
            )
        ) {
            continue;
        }
        match probe_live_tunnel() {
            Some(live) => {
                if current != Some(UiStatus::Up) {
                    emit_status(&app, &state, UiStatus::Up);
                    let iface = live.iface.as_deref().unwrap_or("ppp?");
                    push_log(
                        &app,
                        &state,
                        format!(
                            "tofv: tunnel encore actif (pid {}, {iface}) — l'UI précédente a quitté sans Couper",
                            live.pid
                        ),
                    );
                }
            }
            None => {
                if current == Some(UiStatus::Up) {
                    emit_status(&app, &state, UiStatus::Idle);
                    push_log(&app, &state, "tofv: tunnel disparu".into());
                }
            }
        }
    }
}

fn main() {
    singleton::ignore_hup_and_detach_tty();

    // Before GTK init: let KWin draw SSD. GTK CSD on Plasma/Wayland
    // leaves min/max/close unclickable until the window is maximized.
    #[cfg(target_os = "linux")]
    if std::env::var_os("GTK_CSD").is_none() {
        std::env::set_var("GTK_CSD", "0");
    }

    let state = AppState::new().unwrap_or_else(|e| {
        eprintln!("tofv-app: {e}");
        std::process::exit(1);
    });
    let sock = state.paths.app_socket_path();
    let singleton_listener = match singleton::claim(&sock) {
        Ok(singleton::Claim::AlreadyRunning) => {
            return;
        }
        Ok(singleton::Claim::Server(l)) => Some(l),
        Err(e) => {
            eprintln!("tofv-app: instance unique: {e}");
            None
        }
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_state,
            save_profile,
            save_password,
            clear_password,
            preview,
            connect,
            begin_connect,
            disconnect_cmd,
            trust_cert,
            open_panel,
            open_journal
        ])
        .setup(move |app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            if let Some(listener) = singleton_listener {
                singleton::serve(listener, app.handle().clone());
            }

            if appindicator_available() {
                let open =
                    MenuItem::with_id(app, "open", "Ouvrir le panneau", true, None::<&str>)?;
                let connect =
                    MenuItem::with_id(app, "connect", "Connecter…", true, None::<&str>)?;
                let disconnect =
                    MenuItem::with_id(app, "disconnect", "Déconnecter", true, None::<&str>)?;
                let quit = MenuItem::with_id(app, "quit", "Quitter", true, None::<&str>)?;
                let menu = Menu::with_items(app, &[&open, &connect, &disconnect, &quit])?;
                let icon = Image::from_bytes(TRAY_ICON_PNG)?;
                TrayIconBuilder::with_id("main")
                    .icon(icon)
                    .tooltip("TOFV — déconnecté")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| match event.id.as_ref() {
                        "open" => show_panel(app),
                        "connect" => {
                            let state = app.state::<AppState>();
                            if let Err(e) = begin_connect_inner(app, &state) {
                                push_log(app, &state, format!("tofv: {e}"));
                                let _ = app.emit("tofv://toast", e);
                                show_panel(app);
                            }
                        }
                        "disconnect" => {
                            let state = app.state::<AppState>();
                            let _ = disconnect_cmd(app.clone(), state);
                        }
                        "quit" => {
                            let state = app.state::<AppState>();
                            tear_down_vpn(&state);
                            app.exit(0);
                        }
                        _ => {}
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_panel(tray.app_handle());
                        }
                    })
                    .build(app)?;
                if !singleton::tray_only() {
                    show_panel(app.handle());
                }
            } else {
                eprintln!(
                    "tofv-app: libayatana-appindicator manquante — panneau seul (sudo pacman -S libayatana-appindicator)"
                );
                show_panel(app.handle());
            }

            if let Some(win) = app.get_webview_window("main") {
                let _ = win.set_icon(Image::from_bytes(WINDOW_ICON_PNG)?);
            }

            {
                let state = app.state::<AppState>();
                let _ = adopt_live_tunnel(app.handle(), &state);
            }

            let poll_handle = app.handle().clone();
            thread::spawn(move || poller(poll_handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" || window.label() == "otp" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("tofv-app failed to start")
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } = event {
                let state = app.state::<AppState>();
                tear_down_vpn(&state);
            }
        });
}

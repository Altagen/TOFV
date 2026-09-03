import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { bindChrome } from "./chrome";

type UiStatus =
  | "idle"
  | "connecting"
  | "up"
  | "disconnecting"
  | "need-cert"
  | "auth-failed"
  | "error";

type Snapshot = {
  status: UiStatus;
  profile: {
    host: string;
    port: number;
    username: string;
    realm: string;
    trustedCert: string | null;
    authMethod: string;
    hasPassword: boolean;
  };
  logs: string[];
  command: string | null;
  configRedacted: string | null;
  needCert: string | null;
  lastError: string | null;
  doctor: {
    blocking: boolean;
    helperOk: boolean;
    trayOk: boolean;
    installCmd: string;
    helperCmd: string;
    items: {
      id: string;
      ok: boolean;
      blocking: boolean;
      label: string;
      detail: string;
    }[];
  };
};

const TAIL = 10;
let tail: string[] = [];
let toastTimer = 0;

const $ = <T extends HTMLElement>(id: string) =>
  document.getElementById(id) as T;

let tailFrame = 0;

function paintTail() {
  // `openfortivpn -v` logs a line per packet: repaint once per frame, not
  // once per line, or the webview spins re-joining the whole tail.
  if (tailFrame) return;
  tailFrame = requestAnimationFrame(() => {
    tailFrame = 0;
    $("journal").textContent = tail.length ? tail.join("\n") : "waiting.";
  });
}

function pushTail(line: string) {
  tail.push(line);
  if (tail.length > TAIL) tail = tail.slice(-TAIL);
  paintTail();
}

const labels: Record<UiStatus, string> = {
  idle: "disconnected",
  connecting: "connecting…",
  disconnecting: "disconnecting…",
  up: "connected",
  "need-cert": "certificate to pin",
  "auth-failed": "auth rejected",
  error: "error",
};

// The password is write-only: it goes to the keyring and is never read back
// into the form. Showing an empty input in both states made "stored" and
// "not stored" look identical, so the field only exists while there is
// nothing stored, or while the user asked to replace it.
let replacingPassword = false;

function renderPassword(hasPassword: boolean) {
  const stored = hasPassword && !replacingPassword;
  $("pass-entry").hidden = stored;
  $("pass-stored").hidden = !stored;
  if (stored) {
    $("pass-stored-label").textContent = "Stored in your keyring (dev.tofv / default)";
    $("pass-state").textContent =
      "Connect reads it from the keyring. TOFV never writes it to disk.";
    $("pass-state").classList.remove("error");
    return;
  }
  $("pass-state").textContent = hasPassword
    ? "Type a new password to replace the stored one."
    : "Stored once in your desktop keyring. TOFV never writes it to disk.";
  $("pass-state").classList.remove("error");
}

// Preview is meaningless while a session is live: the command block is then
// showing the real invocation, and the backend refuses to overwrite it.
function setPreviewEnabled(enabled: boolean) {
  const btn = $("btn-preview") as HTMLButtonElement;
  btn.disabled = !enabled;
  btn.title = enabled
    ? "Show the exact command and redacted config TOFV would run, without connecting"
    : "Not available while a session is running — the command shown is the real one";
}

function render(s: Snapshot) {
  const led = $("led");
  led.className = `led ${s.status}`;
  led.dataset.status = s.status;
  $("status-label").textContent = labels[s.status] ?? s.status;

  ($("host") as HTMLInputElement).value = s.profile.host;
  ($("port") as HTMLInputElement).value = String(s.profile.port);
  ($("username") as HTMLInputElement).value = s.profile.username;
  ($("realm") as HTMLInputElement).value = s.profile.realm;
  ($("trusted") as HTMLInputElement).value = s.profile.trustedCert ?? "";
  renderPassword(s.profile.hasPassword);

  applyDoctor(s.doctor);

  const byId = (id: string) => s.doctor.items.find((i) => i.id === id);
  $("doc-vpn").textContent = byId("openfortivpn")?.detail ?? "…";
  $("doc-secret").textContent = byId("secret-tool")?.detail ?? "…";
  $("doc-helper").textContent = byId("helper")?.detail ?? "…";

  // The same planner fills this block for a Preview click and for a real
  // connection, so the label has to say which one you are looking at.
  const live =
    s.status === "connecting" || s.status === "up" || s.status === "disconnecting";
  $("command-kicker").textContent = live ? "command running" : "command preview";
  $("command").textContent = s.command ?? "—";
  tail = s.logs.slice(-TAIL);
  paintTail();

  const busy =
    s.status === "up" || s.status === "connecting" || s.status === "disconnecting";
  ($("btn-connect") as HTMLButtonElement).disabled = busy;
  ($("btn-disconnect") as HTMLButtonElement).disabled =
    s.status === "idle" || s.status === "disconnecting";
  setPreviewEnabled(!busy);

  if (s.needCert) {
    showCertModal(s.needCert, s.profile.trustedCert);
  }
}

function applyDoctor(d: Snapshot["doctor"]) {
  const gate = $("gate");
  const banner = $("banner-helper");
  const mast = document.querySelector("header.mast") as HTMLElement | null;
  const scroll = document.querySelector(".scroll") as HTMLElement | null;
  gate.hidden = !d.blocking;
  if (mast) mast.hidden = d.blocking;
  if (scroll) scroll.hidden = d.blocking;
  banner.hidden = d.blocking || d.helperOk;

  const list = $("gate-list");
  list.replaceChildren();
  for (const item of d.items.filter((i) => !i.ok)) {
    const li = document.createElement("li");
    li.className = item.blocking ? "bad" : "warn";
    li.textContent = `${item.label} : ${item.detail}`;
    list.appendChild(li);
  }
  $("gate-cmd").textContent = d.installCmd;
  $("gate-helper").textContent = d.helperCmd;
}

function showCertModal(sha256: string, previous: string | null) {
  $("cert-hex").textContent = sha256;
  const rotated = !!(previous && previous !== sha256);
  $("cert-old-block").hidden = !rotated;
  $("cert-old").textContent = previous ?? "";
  $("cert-title").textContent = rotated
    ? "The gateway certificate changed"
    : "Trust this certificate?";
  $("cert-lede").textContent = rotated
    ? "Likely a rotation. Compare the old and new fingerprints, then pin and reconnect (fresh code)."
    : "openfortivpn rejects this SHA-256 fingerprint. Verify it before pinning.";
  $("modal-cert").hidden = false;
}

async function refresh() {
  const s = await invoke<Snapshot>("get_state");
  render(s);
  return s;
}

function syncOtpButton() {
  const otp = ($("otp") as HTMLInputElement).value.trim();
  ($("otp-go") as HTMLButtonElement).disabled = !/^\d{6}$/.test(otp);
}

function toast(msg: string) {
  const el = $("toast");
  el.textContent = msg;
  el.hidden = false;
  window.clearTimeout(toastTimer);
  toastTimer = window.setTimeout(() => {
    el.hidden = true;
  }, 7000);
}

function closeOtp() {
  $("modal-otp").hidden = true;
}

function openOtp(error?: string | null) {
  $("modal-otp").hidden = false;
  $("otp-error").textContent = error ?? "";
  const otp = $("otp") as HTMLInputElement;
  otp.value = "";
  syncOtpButton();
  otp.focus();
}

async function startConnect() {
  try {
    await invoke("begin_connect");
  } catch (err) {
    toast(String(err));
    pushTail(`tofv: ${err}`);
  }
}

async function doConnect(otp: string) {
  const errEl = $("otp-error");
  errEl.textContent = "";
  const btn = $("otp-go") as HTMLButtonElement;
  btn.disabled = true;
  try {
    await invoke("connect", { otp });
    await refresh();
  } catch (e) {
    const msg = e instanceof Error ? e.message : String(e);
    errEl.textContent = msg;
    pushTail(`tofv: ${msg}`);
  } finally {
    btn.disabled = false;
  }
}

function needPassword() {
  toast("No password in the keyring — type one, then Store in keyring.");
  replacingPassword = true;
  renderPassword(false);
  $("pass-state").textContent = "Connect needs a stored password.";
  $("pass-state").classList.add("error");
  ($("password") as HTMLInputElement).focus();
}

window.addEventListener("DOMContentLoaded", async () => {
  bindChrome("hide");
  $("gate-recheck").addEventListener("click", () => {
    void refresh();
  });

  $("profile-form").addEventListener("submit", async (e) => {
    e.preventDefault();
    try {
      await invoke("save_profile", {
        patch: {
          host: ($("host") as HTMLInputElement).value,
          port: Number(($("port") as HTMLInputElement).value),
          username: ($("username") as HTMLInputElement).value,
          realm: ($("realm") as HTMLInputElement).value,
          trustedCert: ($("trusted") as HTMLInputElement).value || null,
        },
      });
      await invoke("preview");
      await refresh();
      toast("Profile saved");
    } catch (err) {
      // Without this the form just sat there: the only trace of a rejected
      // profile was a line in the log tail.
      toast(`Could not save the profile: ${err}`);
      pushTail(`tofv: ${err}`);
    }
  });

  $("btn-save-pass").addEventListener("click", async () => {
    const input = $("password") as HTMLInputElement;
    if (!input.value) {
      $("pass-state").textContent = "Type the VPN password first.";
      $("pass-state").classList.add("error");
      input.focus();
      return;
    }
    try {
      await invoke("save_password", { password: input.value });
      input.value = "";
      replacingPassword = false;
      await refresh();
    } catch (err) {
      $("pass-state").textContent = String(err);
      $("pass-state").classList.add("error");
      pushTail(`tofv: ${err}`);
    }
  });

  $("btn-replace-pass").addEventListener("click", () => {
    replacingPassword = true;
    renderPassword(true);
    ($("password") as HTMLInputElement).focus();
  });

  $("btn-forget-pass").addEventListener("click", async () => {
    try {
      await invoke("clear_password");
      replacingPassword = false;
      ($("password") as HTMLInputElement).value = "";
      await refresh();
      pushTail("tofv: password removed from the keyring");
    } catch (err) {
      $("pass-state").textContent = String(err);
      $("pass-state").classList.add("error");
      pushTail(`tofv: ${err}`);
    }
  });

  $("btn-connect").addEventListener("click", () => {
    void startConnect();
  });
  $("btn-disconnect").addEventListener("click", async () => {
    try {
      await invoke("disconnect_cmd");
      await refresh();
    } catch (err) {
      pushTail(`tofv: ${err}`);
    }
  });
  $("btn-preview").addEventListener("click", async () => {
    await invoke("preview");
    await refresh();
  });
  $("btn-journal").addEventListener("click", async () => {
    try {
      await invoke("open_journal");
    } catch (err) {
      pushTail(`tofv: ${err}`);
    }
  });

  $("otp-cancel").addEventListener("click", () => closeOtp());
  $("otp-go").addEventListener("click", async () => {
    const otp = ($("otp") as HTMLInputElement).value.trim();
    if (!/^\d{6}$/.test(otp)) {
      $("otp-error").textContent = "The code must be exactly 6 digits.";
      return;
    }
    await doConnect(otp);
  });
  $("otp").addEventListener("input", () => {
    const el = $("otp") as HTMLInputElement;
    el.value = el.value.replace(/\D/g, "").slice(0, 6);
    syncOtpButton();
  });
  $("otp").addEventListener("paste", (e) => {
    const text = e.clipboardData?.getData("text") ?? "";
    const digits = text.replace(/\D/g, "").slice(0, 6);
    if (digits) {
      e.preventDefault();
      ($("otp") as HTMLInputElement).value = digits;
      syncOtpButton();
    }
  });
  $("otp").addEventListener("keydown", (e) => {
    if (e.key === "Enter") $("otp-go").click();
  });
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("modal-otp").hidden) closeOtp();
    if (e.key === "Escape" && !$("modal-cert").hidden) $("modal-cert").hidden = true;
  });

  $("cert-cancel").addEventListener("click", () => {
    $("modal-cert").hidden = true;
  });
  $("cert-trust").addEventListener("click", async () => {
    const cert = $("cert-hex").textContent ?? "";
    try {
      await invoke("trust_cert", { cert });
      $("modal-cert").hidden = true;
      await refresh();
      await invoke("begin_connect");
    } catch (err) {
      pushTail(`tofv: ${err}`);
    }
  });

  await listen<string[]>("tofv://log", (ev) => {
    for (const line of ev.payload) tail.push(line);
    if (tail.length > TAIL) tail = tail.slice(-TAIL);
    paintTail();
  });
  await listen<UiStatus>("tofv://status", (ev) => {
    const led = $("led");
    led.className = `led ${ev.payload}`;
    $("status-label").textContent = labels[ev.payload] ?? ev.payload;
    const busy =
      ev.payload === "up" ||
      ev.payload === "connecting" ||
      ev.payload === "disconnecting";
    ($("btn-connect") as HTMLButtonElement).disabled = busy;
    ($("btn-disconnect") as HTMLButtonElement).disabled =
      ev.payload === "idle" || ev.payload === "disconnecting";
    setPreviewEnabled(!busy);
    if (ev.payload === "auth-failed" && !$("modal-otp").hidden) {
      openOtp(
        "Code rejected. The token rotates about every 60 s — enter the code shown right now.",
      );
    }
    if (ev.payload === "up" || ev.payload === "need-cert") closeOtp();
  });
  await listen<string>("tofv://ask-otp-fallback", (ev) => {
    openOtp((ev.payload ?? "").trim() || null);
  });
  await listen("tofv://need-password", () => needPassword());
  await listen<string>("tofv://toast", (ev) => {
    if (ev.payload) toast(ev.payload);
  });
  await listen<{ sha256: string; previous: string | null }>("tofv://need-cert", (ev) => {
    showCertModal(ev.payload.sha256, ev.payload.previous);
  });

  try {
    await refresh();
  } catch (err) {
    pushTail(String(err));
  }
});

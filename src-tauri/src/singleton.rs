//! One running `tofv-app` per user. A second launch asks the first to show
//! the panel, then exits. Closing the launching terminal must not kill us.

use std::ffi::CString;
use std::io::{BufRead, BufReader, IsTerminal, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::thread;

use tauri::{AppHandle, Manager};

pub enum Claim {
    Server(UnixListener),
    AlreadyRunning,
}

/// From a TTY, fork so the shell gets its prompt back and closing the
/// terminal cannot SIGHUP us. `--foreground` / `-f` keeps the job in
/// the terminal (logs). `.desktop` launches are not TTYs: no fork.
pub fn ignore_hup_and_detach_tty() {
    let foreground = std::env::args().any(|a| a == "--foreground" || a == "-f");
    if foreground {
        return;
    }
    let from_tty = std::io::stdin().is_terminal() || std::io::stdout().is_terminal();
    if from_tty {
        let pid = unsafe { libc::fork() };
        if pid > 0 {
            std::process::exit(0);
        }
        if pid == 0 {
            unsafe {
                libc::setsid();
            }
            redirect_stdio_null();
        }
    }
    unsafe {
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
    }
}

fn redirect_stdio_null() {
    let path = CString::new("/dev/null").expect("/dev/null");
    let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR) };
    if fd < 0 {
        return;
    }
    unsafe {
        libc::dup2(fd, libc::STDIN_FILENO);
        libc::dup2(fd, libc::STDOUT_FILENO);
        libc::dup2(fd, libc::STDERR_FILENO);
        if fd > 2 {
            libc::close(fd);
        }
    }
}

pub fn claim(path: &Path) -> std::io::Result<Claim> {
    if let Ok(mut stream) = UnixStream::connect(path) {
        let _ = stream.write_all(b"SHOW\n");
        let _ = stream.flush();
        return Ok(Claim::AlreadyRunning);
    }
    let _ = std::fs::remove_file(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let listener = UnixListener::bind(path)?;
    // bind() applies the umask; pin it to the owner so the "show the panel"
    // channel matches the 0700 runtime dir it lives in.
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(Claim::Server(listener))
}

pub fn serve(listener: UnixListener, app: AppHandle) {
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let mut line = String::new();
            if BufReader::new(stream).read_line(&mut line).is_err() {
                continue;
            }
            if line.trim() == "SHOW" {
                let handle = app.clone();
                let shown = handle.clone();
                let _ = handle.run_on_main_thread(move || {
                    if let Some(w) = shown.get_webview_window("main") {
                        let _ = w.show();
                        let _ = w.unminimize();
                        let _ = w.set_focus();
                    }
                });
            }
        }
    });
}

pub fn tray_only() -> bool {
    std::env::args().any(|a| a == "--tray" || a == "--hidden")
}

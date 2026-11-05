use crate::json::SessionInfo;
use crate::util::{
    configure_child, ensure_private_dir, find_in_path, read_tail, run_capped, tcp_listening_local,
    terminate_pid,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub fn binary() -> Result<PathBuf, String> {
    if let Ok(path) = std::env::var("NIRUREMOTE_X11VNC") {
        let path = PathBuf::from(path);
        if path.exists() {
            return Ok(path);
        }
        return Err(format!(
            "NIRUREMOTE_X11VNC does not exist: {}",
            path.display()
        ));
    }
    find_in_path("x11vnc").ok_or_else(|| "x11vnc is not installed".into())
}

pub fn version(path: &Path) -> Option<String> {
    let output = run_capped(
        &path.display().to_string(),
        &["-version"],
        Duration::from_secs(2),
        64 * 1024,
    )
    .ok()?;
    let text = format!("{}\n{}", output.stdout_text(), output.stderr_text());
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

pub fn install_hint() -> String {
    let release = fs::read_to_string("/etc/os-release").unwrap_or_default();
    let mut id = String::new();
    let mut like = String::new();
    for line in release.lines() {
        if let Some(value) = line.strip_prefix("ID=") {
            id = value.trim_matches('"').to_lowercase();
        } else if let Some(value) = line.strip_prefix("ID_LIKE=") {
            like = value.trim_matches('"').to_lowercase();
        }
    }
    hint_for(&id, &like).into()
}

fn hint_for(id: &str, like: &str) -> &'static str {
    let family = format!("{id} {like}");
    if family.contains("arch") {
        "sudo pacman -S --needed x11vnc tigervnc"
    } else if family.contains("void") {
        "sudo xbps-install -Sy x11vnc tigervnc"
    } else if family.contains("fedora") || family.contains("rhel") {
        "sudo dnf install -y x11vnc tigervnc"
    } else if family.contains("suse") {
        "sudo zypper install -y x11vnc tigervnc"
    } else if family.contains("debian") || family.contains("ubuntu") {
        "sudo apt-get install -y x11vnc tigervnc-viewer"
    } else {
        "Install x11vnc and a VNC viewer with your package manager"
    }
}

/// Options shared by every x11vnc we start.
///
/// `-norc` ignores ~/.x11vncrc, `-nocmds` stops x11vnc from running external
/// commands, `-noremote` ignores X11VNC_REMOTE control properties and
/// `-noipv6` keeps it from opening its default `[::1]:5900` listener.
fn base_args(display: &str, desktop: &str, auth: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = [
        "-norc",
        "-display",
        display,
        "-nocmds",
        "-noremote",
        "-noipv6",
        "-noxdamage",
        "-repeat",
        "-quiet",
        "-desktop",
        desktop,
    ]
    .iter()
    .map(|argument| argument.to_string())
    .collect();
    if let Some(auth) = auth {
        args.push("-auth".into());
        args.push(auth.into());
    }
    args
}

/// x11vnc for one `connect`: RFB over stdin/stdout, no listening socket.
/// `-inetd` serves exactly one viewer and exits when its stream closes.
/// `-ping` sends a 1x1 update every few seconds so the viewer keeps answering;
/// the agent uses that traffic to notice a dead network path.
pub fn stream_args(display: &str, desktop: &str, auth: Option<&str>) -> Vec<String> {
    let mut args = base_args(display, desktop, auth);
    args.extend(
        ["-inetd", "-nopw", "-rfbport", "0", "-ping", "5"]
            .iter()
            .map(|argument| argument.to_string()),
    );
    args
}

/// x11vnc for `enable`: a loopback-only listener protected by a per-enable
/// VNC password that x11vnc reads from a 0600 file and then deletes (`rm:`).
/// One viewer at a time; a second one is refused rather than sharing.
pub fn listener_args(
    display: &str,
    desktop: &str,
    auth: Option<&str>,
    port: u16,
    password_file: &Path,
) -> Vec<String> {
    let mut args = base_args(display, desktop, auth);
    args.extend([
        "-localhost".to_string(),
        "-rfbport".to_string(),
        port.to_string(),
        // libvncserver otherwise adds [::1]:5900 even with -noipv6.
        "-rfbportv6".to_string(),
        "-1".to_string(),
        "-passwdfile".to_string(),
        format!("rm:{}", password_file.display()),
        "-forever".to_string(),
        "-nevershared".to_string(),
        "-dontdisconnect".to_string(),
    ]);
    args
}

pub fn desktop_name() -> String {
    // ASCII only: the RFB desktop name has no defined encoding.
    let host: String = crate::util::hostname()
        .chars()
        .filter(|character| character.is_ascii_graphic())
        .take(48)
        .collect();
    format!("NIRUREMOTE - {host}")
}

fn open_log(log_path: &Path) -> Result<(fs::File, fs::File), String> {
    prepare_log(log_path)?;
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .map_err(|error| format!("x11vnc log: {error}"))?;
    let _ = fs::set_permissions(log_path, fs::Permissions::from_mode(0o600));
    let log_err = log
        .try_clone()
        .map_err(|error| format!("x11vnc log: {error}"))?;
    Ok((log, log_err))
}

/// Start x11vnc speaking RFB on `stream`. It dies with the agent
/// (`PR_SET_PDEATHSIG`) and when the stream reaches end-of-file.
pub fn spawn_stream(
    session: &SessionInfo,
    stream: std::os::unix::net::UnixStream,
    log_path: &Path,
) -> Result<Child, String> {
    let display = session
        .display
        .as_deref()
        .ok_or_else(|| "no X11 display to attach to".to_string())?;
    let binary = binary()?;
    let (_log, log_err) = open_log(log_path)?;
    let output = stream
        .try_clone()
        .map_err(|error| format!("x11vnc stream: {error}"))?;
    let mut command = Command::new(&binary);
    command
        .args(stream_args(
            display,
            &desktop_name(),
            session.auth_file.as_deref(),
        ))
        .stdin(Stdio::from(std::os::fd::OwnedFd::from(stream)))
        .stdout(Stdio::from(std::os::fd::OwnedFd::from(output)))
        .stderr(Stdio::from(log_err));
    configure_child(&mut command, false, true);
    command
        .spawn()
        .map_err(|error| format!("could not start x11vnc: {error}"))
}

pub fn start_listener(
    session: &SessionInfo,
    port: u16,
    password_file: &Path,
    log_path: &Path,
) -> Result<u32, String> {
    let display = session
        .display
        .as_deref()
        .ok_or_else(|| "no X11 display to attach to".to_string())?;
    let binary = binary()?;
    let (log, log_err) = open_log(log_path)?;
    let mut command = Command::new(&binary);
    command
        .args(listener_args(
            display,
            &desktop_name(),
            session.auth_file.as_deref(),
            port,
            password_file,
        ))
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    configure_child(&mut command, true, false);

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start x11vnc: {error}"))?;
    let pid = child.id();

    // Bounded start-up wait. Readiness is read from /proc/net/tcp, so it
    // never opens a VNC connection.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let tail = read_tail(log_path, 4096).unwrap_or_default();
            return Err(format!(
                "x11vnc exited with {status} while starting: {tail}"
            ));
        }
        if tcp_listening_local(port) {
            // Detached on purpose: it outlives this command until `disable`.
            drop(child);
            return Ok(pid);
        }
        if Instant::now() >= deadline {
            terminate_pid(pid, Duration::from_secs(1));
            let _ = child.wait();
            let tail = read_tail(log_path, 4096).unwrap_or_default();
            return Err(format!(
                "x11vnc did not open 127.0.0.1:{port} within 5s: {tail}"
            ));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn stop(pid: u32) {
    if !crate::util::process_matches(pid, "x11vnc") {
        return;
    }
    terminate_pid(pid, Duration::from_secs(3));
}

fn prepare_log(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent).map_err(|error| format!("state directory: {error}"))?;
    }
    if fs::metadata(path).is_ok_and(|metadata| metadata.len() > 1024 * 1024) {
        let rotated = path.with_extension("log.1");
        let _ = fs::remove_file(&rotated);
        let _ = fs::rename(path, rotated);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_hint_matches_distribution_family() {
        assert!(hint_for("ubuntu", "debian").contains("apt-get"));
        assert!(hint_for("arch", "").contains("pacman"));
        assert!(hint_for("void", "").contains("xbps-install"));
        assert!(hint_for("fedora", "").contains("dnf"));
        assert!(hint_for("debian", "ubuntu debian").contains("tigervnc-viewer"));
    }

    fn has(args: &[String], flag: &str) -> bool {
        args.iter().any(|argument| argument == flag)
    }

    #[test]
    fn stream_mode_has_no_listener_and_no_leftover_flags() {
        let args = stream_args(":0", "NIRUREMOTE", Some("/run/user/1000/xauth"));
        assert!(has(&args, "-inetd"));
        assert_eq!(
            args.windows(2)
                .find(|pair| pair[0] == "-rfbport")
                .map(|pair| pair[1].as_str()),
            Some("0")
        );
        for flag in ["-forever", "-shared", "-many", "-listen", "-http", "-6"] {
            assert!(!has(&args, flag), "{flag} must not be used");
        }
        for flag in ["-norc", "-nocmds", "-noremote", "-noipv6", "-ping"] {
            assert!(has(&args, flag), "{flag} is required");
        }
    }

    #[test]
    fn listener_mode_is_loopback_and_password_protected() {
        let args = listener_args(":0", "NIRUREMOTE", None, 43123, Path::new("/x/pw"));
        assert!(has(&args, "-localhost"));
        assert!(has(&args, "-nevershared"));
        assert!(has(&args, "-dontdisconnect"));
        assert!(has(&args, "rm:/x/pw"));
        for flag in ["-nopw", "-shared", "-passwd", "-rfbauth", "-6", "-listen"] {
            assert!(!has(&args, flag), "{flag} must not be used");
        }
        // The password itself is never an argument.
        assert!(!args.iter().any(|argument| argument.contains("passwd ")));
    }
}

use crate::json::SessionInfo;
use crate::util::{
    configure_child, ensure_private_dir, find_in_path, port_open_local, read_tail, run_capped,
    terminate_pid,
};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartMode {
    Detached,
    Held,
}

pub struct Started {
    pub pid: u32,
    pub child: Option<Child>,
}

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

pub fn start(
    session: &SessionInfo,
    port: u16,
    mode: StartMode,
    log_path: &Path,
) -> Result<Started, String> {
    let display = session
        .display
        .as_deref()
        .ok_or_else(|| "no X11 display to attach to".to_string())?;
    let binary = binary()?;
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

    let mut args = vec![
        "-norc".to_string(),
        "-display".to_string(),
        display.to_string(),
        "-localhost".to_string(),
        "-rfbport".to_string(),
        port.to_string(),
        "-nopw".to_string(),
        "-forever".to_string(),
        "-shared".to_string(),
        "-noxdamage".to_string(),
        "-repeat".to_string(),
        "-quiet".to_string(),
        "-desktop".to_string(),
        "NIRUREMOTE".to_string(),
    ];
    if let Some(auth) = &session.auth_file {
        args.push("-auth".to_string());
        args.push(auth.clone());
    }

    let mut command = Command::new(&binary);
    command
        .args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(log_err));
    configure_child(
        &mut command,
        mode == StartMode::Detached,
        mode == StartMode::Held,
    );

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start x11vnc: {error}"))?;
    let pid = child.id();

    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let tail = read_tail(log_path, 4096).unwrap_or_default();
            return Err(format!(
                "x11vnc exited with {status} while starting: {tail}"
            ));
        }
        if port_open_local(port, Duration::from_millis(200)) {
            return Ok(Started {
                pid,
                child: (mode == StartMode::Held).then_some(child),
            });
        }
        if Instant::now() >= deadline {
            terminate_pid(pid, Duration::from_secs(1));
            let tail = read_tail(log_path, 4096).unwrap_or_default();
            return Err(format!(
                "x11vnc did not open 127.0.0.1:{port} within 4s: {tail}"
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
}

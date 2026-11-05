use crate::util::{configure_child, find_in_path, run_capped};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum VncViewer {
    TigerVnc(PathBuf),
    Remmina(PathBuf),
}

#[derive(Clone, Debug)]
pub enum RdpViewer {
    FreeRdp(PathBuf),
    Remmina(PathBuf),
}

pub fn find_vnc() -> Option<VncViewer> {
    if let Ok(path) = std::env::var("NIRUREMOTE_VIEWER") {
        let path = PathBuf::from(path);
        if path.exists() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("remmina"))
            {
                return Some(VncViewer::Remmina(path));
            }
            return Some(VncViewer::TigerVnc(path));
        }
    }
    for name in ["vncviewer", "xtigervncviewer", "tigervncviewer"] {
        if let Some(path) = find_in_path(name) {
            return Some(VncViewer::TigerVnc(path));
        }
    }
    find_in_path("remmina").map(VncViewer::Remmina)
}

pub fn find_rdp() -> Option<RdpViewer> {
    for name in ["xfreerdp3", "xfreerdp", "wlfreerdp3", "wlfreerdp"] {
        if let Some(path) = find_in_path(name) {
            return Some(RdpViewer::FreeRdp(path));
        }
    }
    find_in_path("remmina").map(RdpViewer::Remmina)
}

/// Whether this viewer can connect to a Unix socket path. TigerVNC 1.12+
/// lists "[unix socket]" in its usage text.
pub fn supports_unix_socket(viewer: &VncViewer) -> bool {
    let VncViewer::TigerVnc(binary) = viewer else {
        return false;
    };
    if std::env::var_os("NIRUREMOTE_VIEWER_UNIX").is_some() {
        return true;
    }
    let Ok(output) = run_capped(
        &binary.display().to_string(),
        &["-h"],
        Duration::from_secs(3),
        64 * 1024,
    ) else {
        return false;
    };
    format!("{}{}", output.stdout_text(), output.stderr_text())
        .to_lowercase()
        .contains("unix socket")
}

/// Where the viewer connects for a NIRUREMOTE session: a Unix socket in a
/// private 0700 directory, or (Remmina, old TigerVNC) a loopback TCP port
/// whose single connection is checked to belong to the current user.
pub enum Endpoint<'a> {
    Socket(&'a Path),
    Loopback(u16),
}

pub fn spawn_vnc_session(viewer: &VncViewer, endpoint: Endpoint) -> Result<Child, String> {
    let mut command = match (viewer, endpoint) {
        (VncViewer::TigerVnc(binary), endpoint) => {
            let mut command = Command::new(binary);
            // The SSH channel is the authentication; the endpoint is private.
            command.arg("-SecurityTypes").arg("None");
            match endpoint {
                Endpoint::Socket(path) => command.arg(path),
                Endpoint::Loopback(port) => command.arg(format!("127.0.0.1::{port}")),
            };
            command
        }
        (VncViewer::Remmina(binary), Endpoint::Loopback(port)) => {
            let mut command = Command::new(binary);
            command.arg("-c").arg(format!("vnc://127.0.0.1:{port}"));
            command
        }
        (VncViewer::Remmina(_), Endpoint::Socket(_)) => {
            return Err("Remmina cannot open a Unix socket".into());
        }
    };
    spawn_viewer(&mut command)
}

pub fn spawn_vnc_host(viewer: &VncViewer, host: &str, port: u16) -> Result<Child, String> {
    let mut command = match viewer {
        VncViewer::TigerVnc(binary) => {
            let mut command = Command::new(binary);
            command.arg("-Shared").arg(format!("{host}::{port}"));
            command
        }
        VncViewer::Remmina(binary) => {
            let mut command = Command::new(binary);
            command.arg("-c").arg(format!("vnc://{host}:{port}"));
            command
        }
    };
    spawn_viewer(&mut command)
}

pub fn spawn_rdp(viewer: &RdpViewer, host: &str) -> Result<Child, String> {
    let mut command = match viewer {
        RdpViewer::FreeRdp(binary) => {
            let mut command = Command::new(binary);
            command
                .arg(format!("/v:{host}"))
                .arg("/dynamic-resolution")
                .arg("+clipboard");
            if binary
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.contains("freerdp3"))
            {
                command.arg("/cert:tofu");
            }
            command
        }
        RdpViewer::Remmina(binary) => {
            let mut command = Command::new(binary);
            command.arg("-c").arg(format!("rdp://{host}"));
            command
        }
    };
    spawn_viewer(&mut command)
}

fn spawn_viewer(command: &mut Command) -> Result<Child, String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    configure_child(command, false, true);
    command
        .spawn()
        .map_err(|error| format!("could not start the viewer: {error}"))
}

pub fn vnc_install_hint() -> &'static str {
    "Install TigerVNC viewer: sudo apt-get install tigervnc-viewer (Arch: tigervnc, Void: tigervnc)"
}

pub fn rdp_install_hint() -> &'static str {
    "Install FreeRDP: sudo apt-get install freerdp3-x11 (Arch: freerdp, Void: freerdp)"
}

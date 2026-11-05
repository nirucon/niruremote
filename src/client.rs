use crate::events::{self, Event};
use crate::json::{
    CachedPeer, MODE_STREAM, Peer, PeerView, PeersResponse, RemoteView, SCHEMA, Snapshot, SshView,
};
use crate::ssh::{self, SshError, TempFiles};
use crate::state;
use crate::tailscale;
use crate::util::{
    configure_child, human_age, human_age_opt, now_secs, port_open_ip, sanitize, stop_child,
    tcp_peer_uid, uid, unix_peer_uid,
};
use crate::viewer;
use std::os::fd::OwnedFd;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

pub struct ConnectOptions {
    pub print: bool,
    pub timeout: Duration,
}

pub fn peers(refresh: bool, timeout: Duration) -> Result<PeersResponse, String> {
    crate::util::ensure_ssh_agent_env();
    let snapshot = tailscale::status()?;
    require_running(&snapshot)?;
    let mut cache = state::read_cache();
    let peers = tailscale::all_peers(&snapshot, false);

    if refresh {
        let results = probe_all(&peers, timeout);
        for (id, cached) in results {
            cache.peers.insert(id, cached);
        }
        if let Err(error) = state::write_cache(&cache) {
            eprintln!("niruremote: could not write peer cache: {error}");
        }
    }

    let mut views = Vec::with_capacity(peers.len());
    for peer in peers {
        let cached = cache.peers.get(&peer.id).cloned();
        views.push(build_view(peer, cached));
    }
    views.sort_by(|left, right| {
        right
            .peer
            .online
            .cmp(&left.peer.online)
            .then_with(|| state_rank(&left.remote.state).cmp(&state_rank(&right.remote.state)))
            .then_with(|| left.peer.host.cmp(&right.peer.host))
    });

    Ok(PeersResponse {
        schema: SCHEMA,
        refreshed_at: now_secs(),
        peers: views,
    })
}

fn require_running(snapshot: &Snapshot) -> Result<(), String> {
    if snapshot.backend_state == "Running" {
        Ok(())
    } else {
        Err(format!(
            "Tailscale backend is {}; start Tailscale and try again",
            snapshot.backend_state
        ))
    }
}

fn state_rank(state: &str) -> u8 {
    match state {
        "ready" | "client-ready" => 0,
        "starting" | "inactive" => 1,
        "no-local-client" | "not-installed" | "backend-missing" | "update-needed" => 2,
        "no-session" | "unsupported" | "unsupported-wayland" => 3,
        "offline" => 4,
        _ => 5,
    }
}

fn probe_all(peers: &[Peer], timeout: Duration) -> Vec<(String, CachedPeer)> {
    let mut handles = Vec::new();
    let mut results = Vec::new();
    for peer in peers {
        if !peer.online {
            results.push((peer.id.clone(), offline_cache(peer, now_secs())));
            continue;
        }
        if !matches!(peer.os.as_str(), "linux" | "windows" | "macOS" | "macos") {
            results.push((peer.id.clone(), unsupported_cache()));
            continue;
        }
        let peer = peer.clone();
        handles.push(std::thread::spawn(move || {
            let cached = probe_one(&peer, timeout);
            (peer.id.clone(), cached)
        }));
    }
    for handle in handles {
        if let Ok(result) = handle.join() {
            results.push(result);
        }
    }
    results
}

fn probe_one(peer: &Peer, timeout: Duration) -> CachedPeer {
    let checked_at = now_secs();
    if !peer.online {
        return offline_cache(peer, checked_at);
    }
    match peer.os.as_str() {
        "linux" => match ssh::probe(&peer.host, timeout) {
            Ok(local) => map_linux(local, checked_at),
            Err(error) => cached_error(error, checked_at),
        },
        "windows" => probe_windows(peer, checked_at),
        "macOS" | "macos" => probe_macos(peer, checked_at),
        _ => unsupported_cache(),
    }
}

/// The oldest agent that serves `stream` (no listening port on the host).
const MIN_AGENT: &str = "0.2.0";

fn version_tuple(version: &str) -> (u64, u64, u64) {
    let mut parts = version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0));
    (
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    )
}

fn agent_supports_stream(local: &crate::json::LocalState) -> bool {
    local.supported_modes.iter().any(|mode| mode == MODE_STREAM)
        && version_tuple(&local.agent_version) >= version_tuple(MIN_AGENT)
}

fn map_linux(local: crate::json::LocalState, checked_at: u64) -> CachedPeer {
    let version = Some(sanitize(&local.agent_version));
    let mode = Some(MODE_STREAM.to_string());
    let display = sanitize(local.session.display.as_deref().unwrap_or("?"));
    let mut extras = Vec::new();
    if let Some(locker) = &local.locker {
        extras.push(format!("screen locked ({})", sanitize(locker)));
    }
    if local.sessions > 0 {
        extras.push(format!("{} active session(s)", local.sessions));
    }
    if local.enabled {
        extras.push("manual-tunnel listener enabled".into());
    }
    let suffix: String = extras.iter().fold(String::new(), |mut text, extra| {
        text.push_str(" · ");
        text.push_str(extra);
        text
    });
    let (state, detail) = match local.state.as_str() {
        "ready" | "inactive" | "starting" if !agent_supports_stream(&local) => (
            "update-needed".into(),
            format!(
                "Agent {} is older than {MIN_AGENT}; update NIRUREMOTE on this machine",
                sanitize(&local.agent_version)
            ),
        ),
        "ready" | "inactive" | "starting" => (
            "ready".into(),
            format!("X11 {display} · connect any time{suffix}"),
        ),
        "backend-missing" => (
            "backend-missing".into(),
            "Agent installed · x11vnc is missing".into(),
        ),
        "no-session" => (
            "no-session".into(),
            "Agent installed · no X11 desktop session".into(),
        ),
        "unsupported-wayland" => (
            "unsupported-wayland".into(),
            "Agent installed · Wayland is not supported yet".into(),
        ),
        other => ("error".into(), format!("Agent state: {}", sanitize(other))),
    };
    CachedPeer {
        state,
        detail,
        mode,
        version,
        ssh_state: "ok".into(),
        ssh_detail: "SSH reachable".into(),
        checked_at,
    }
}

fn cached_error(error: SshError, checked_at: u64) -> CachedPeer {
    CachedPeer {
        state: error.state.clone(),
        detail: error.detail.clone(),
        mode: None,
        version: None,
        ssh_state: error.state,
        ssh_detail: error.detail,
        checked_at,
    }
}

fn offline_cache(peer: &Peer, checked_at: u64) -> CachedPeer {
    let detail = match peer.last_seen_epoch {
        Some(timestamp) => format!("last seen {}", human_age(timestamp)),
        None => "offline".into(),
    };
    CachedPeer {
        state: "offline".into(),
        detail,
        mode: None,
        version: None,
        ssh_state: "offline".into(),
        ssh_detail: "machine is offline".into(),
        checked_at,
    }
}

fn unsupported_cache() -> CachedPeer {
    CachedPeer {
        state: "unsupported".into(),
        detail: "not a remote desktop host".into(),
        mode: None,
        version: None,
        ssh_state: "not-applicable".into(),
        ssh_detail: "not applicable".into(),
        checked_at: now_secs(),
    }
}

fn probe_windows(peer: &Peer, checked_at: u64) -> CachedPeer {
    let Some(ip) = peer.ip.as_deref() else {
        return unknown_cache("no Tailscale IP");
    };
    if !port_open_ip(ip, 3389, Duration::from_secs(2)) {
        return CachedPeer {
            state: "unreachable".into(),
            detail: "RDP port 3389 is closed; enable Remote Desktop on Windows".into(),
            mode: Some("rdp".into()),
            version: None,
            ssh_state: "not-applicable".into(),
            ssh_detail: "RDP host".into(),
            checked_at,
        };
    }
    if viewer::find_rdp().is_some() {
        CachedPeer {
            state: "client-ready".into(),
            detail: "RDP reachable · Windows Remote Desktop".into(),
            mode: Some("rdp".into()),
            version: None,
            ssh_state: "not-applicable".into(),
            ssh_detail: "RDP host".into(),
            checked_at,
        }
    } else {
        CachedPeer {
            state: "no-local-client".into(),
            detail: "RDP reachable · install FreeRDP to connect".into(),
            mode: Some("rdp".into()),
            version: None,
            ssh_state: "not-applicable".into(),
            ssh_detail: "RDP host".into(),
            checked_at,
        }
    }
}

fn probe_macos(peer: &Peer, checked_at: u64) -> CachedPeer {
    let Some(ip) = peer.ip.as_deref() else {
        return unknown_cache("no Tailscale IP");
    };
    if !port_open_ip(ip, 5900, Duration::from_secs(2)) {
        return CachedPeer {
            state: "unreachable".into(),
            detail: "Screen Sharing port 5900 is closed; enable it in macOS".into(),
            mode: Some("vnc".into()),
            version: None,
            ssh_state: "not-applicable".into(),
            ssh_detail: "VNC host".into(),
            checked_at,
        };
    }
    if viewer::find_vnc().is_some() {
        CachedPeer {
            state: "client-ready".into(),
            detail: "Screen Sharing reachable · macOS VNC".into(),
            mode: Some("vnc".into()),
            version: None,
            ssh_state: "not-applicable".into(),
            ssh_detail: "VNC host".into(),
            checked_at,
        }
    } else {
        CachedPeer {
            state: "no-local-client".into(),
            detail: "VNC reachable · install a VNC viewer to connect".into(),
            mode: Some("vnc".into()),
            version: None,
            ssh_state: "not-applicable".into(),
            ssh_detail: "VNC host".into(),
            checked_at,
        }
    }
}

fn unknown_cache(detail: &str) -> CachedPeer {
    CachedPeer {
        state: "unknown".into(),
        detail: detail.into(),
        mode: None,
        version: None,
        ssh_state: "unknown".into(),
        ssh_detail: detail.into(),
        checked_at: now_secs(),
    }
}

fn build_view(peer: Peer, cached: Option<CachedPeer>) -> PeerView {
    let Some(cached) = cached else {
        return PeerView {
            peer,
            ssh: SshView {
                state: "unknown".into(),
                detail: "not checked yet".into(),
            },
            remote: RemoteView {
                state: "unknown".into(),
                detail: "not checked yet".into(),
                mode: None,
                version: None,
                checked_at: None,
            },
        };
    };

    let (state, detail) = if !peer.online {
        (
            "offline".to_string(),
            if cached.state == "offline" {
                cached.detail.clone()
            } else {
                format!(
                    "offline · last known {} ({})",
                    cached.state,
                    human_age(cached.checked_at)
                )
            },
        )
    } else if cached.state == "offline" {
        (
            "unknown".to_string(),
            "online · not checked since it came back".to_string(),
        )
    } else {
        (cached.state.clone(), cached.detail.clone())
    };

    PeerView {
        peer,
        ssh: SshView {
            state: cached.ssh_state,
            detail: cached.ssh_detail,
        },
        remote: RemoteView {
            state,
            detail,
            mode: cached.mode,
            version: cached.version,
            checked_at: Some(cached.checked_at),
        },
    }
}

pub fn print_peers(response: &PeersResponse) {
    if response.peers.is_empty() {
        println!("No Tailscale peers found.");
        return;
    }
    let host_width = response
        .peers
        .iter()
        .map(|view| view.peer.host.len())
        .max()
        .unwrap_or(4)
        .clamp(4, 32);
    let os_width = response
        .peers
        .iter()
        .map(|view| view.peer.os.len())
        .max()
        .unwrap_or(2)
        .clamp(2, 10);
    println!(
        "{:<host_width$} {:<os_width$} {:<18} DETAIL",
        "NAME", "OS", "STATE"
    );
    for view in &response.peers {
        let detail = sanitize(&view.remote.detail);
        let detail = if detail.chars().count() > 110 {
            format!("{}…", detail.chars().take(109).collect::<String>())
        } else {
            detail
        };
        println!(
            "{:<host_width$} {:<os_width$} {:<18} {}",
            sanitize(&view.peer.host),
            sanitize(&view.peer.os),
            sanitize(&view.remote.state),
            detail
        );
    }
    println!("\nchecked: {}", human_age_opt(Some(response.refreshed_at)));
}

pub fn connect(host: &str, options: &ConnectOptions) -> Result<(), String> {
    crate::util::ensure_ssh_agent_env();
    let snapshot = tailscale::status()?;
    let peer = find_peer(&snapshot, host)
        .ok_or_else(|| format!("no Tailscale peer matches {}", sanitize(host)))?;
    let name = sanitize(&peer.host);
    if !peer.online {
        return Err(format!(
            "{name} is offline{}",
            peer.last_seen_epoch
                .map(|timestamp| format!(" (last seen {})", human_age(timestamp)))
                .unwrap_or_default()
        ));
    }

    if options.print {
        print_plan(&peer);
        return Ok(());
    }

    let (sender, receiver) = mpsc::channel();
    events::watch_signals(sender.clone())?;

    match peer.os.as_str() {
        "linux" => connect_linux(&peer, options, sender, receiver),
        "windows" => connect_windows(&peer, sender, receiver),
        "macOS" | "macos" => connect_macos(&peer, sender, receiver),
        other => Err(format!(
            "{name} is a {} host; remote desktop is not supported",
            sanitize(other)
        )),
    }
}

fn find_peer(snapshot: &Snapshot, host: &str) -> Option<Peer> {
    let host = host.trim_end_matches('.');
    snapshot
        .peers
        .iter()
        .chain(snapshot.self_peer.iter())
        .find(|peer| {
            peer.host == host
                || peer.host.split('.').next() == Some(host)
                || peer.dns_name.as_deref() == Some(host)
                || peer.ip.as_deref() == Some(host)
                || peer.id == host
        })
        .cloned()
}

fn print_plan(peer: &Peer) {
    let host = sanitize(&peer.host);
    println!("host: {host}");
    println!("ip:   {}", sanitize(peer.ip.as_deref().unwrap_or("-")));
    println!("os:   {}", sanitize(&peer.os));
    match peer.os.as_str() {
        "linux" => {
            println!("plan:");
            if peer.is_self {
                println!("  niruremote stream            (x11vnc -inetd, no listening port)");
            } else {
                println!("  ssh {host} niruremote probe --json");
                println!("  ssh {host} niruremote stream (x11vnc -inetd over the SSH channel)");
            }
            println!("  <vnc viewer> <private Unix socket>   (one connection, owner-only)");
        }
        "windows" => {
            println!("plan:");
            println!(
                "  <freerdp> /v:{}",
                sanitize(peer.ip.as_deref().unwrap_or(&peer.host))
            );
        }
        "macOS" | "macos" => {
            println!("plan:");
            println!(
                "  <vnc viewer> {}::5900",
                sanitize(peer.ip.as_deref().unwrap_or(&peer.host))
            );
        }
        _ => println!("plan: unsupported"),
    }
}

/// The local endpoint the viewer connects to. It accepts exactly one
/// connection, from the current user, and is gone before the session starts.
enum LocalListener {
    Unix(std::os::unix::net::UnixListener, std::path::PathBuf),
    Tcp(std::net::TcpListener, u16),
}

impl Drop for LocalListener {
    fn drop(&mut self) {
        if let LocalListener::Unix(_, path) = self {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn accept_one(listener: LocalListener, sender: mpsc::Sender<Event>) {
    std::thread::spawn(move || {
        let result = match &listener {
            LocalListener::Unix(socket, _) => match socket.accept() {
                Ok((stream, _)) => {
                    if unix_peer_uid(&stream) == Some(uid()) {
                        Ok(OwnedFd::from(stream))
                    } else {
                        Err("a different user connected to the viewer socket".to_string())
                    }
                }
                Err(error) => Err(format!("viewer socket: {error}")),
            },
            LocalListener::Tcp(socket, port) => match socket.accept() {
                Ok((stream, peer)) => {
                    if tcp_peer_uid(peer.port(), *port) == Some(uid()) {
                        Ok(OwnedFd::from(stream))
                    } else {
                        Err("a different user connected to the viewer port".to_string())
                    }
                }
                Err(error) => Err(format!("viewer port: {error}")),
            },
        };
        // Close and unlink the listener before handing the stream over.
        drop(listener);
        let _ = sender.send(Event::Accepted(result));
    });
}

/// Remove `view-PID.sock` and `stream-PID-*` left by a client that was
/// killed with SIGKILL (Drop never ran). Only entries of dead PIDs.
fn remove_stale_runtime(directory: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let pid = if let Some(rest) = name.strip_prefix("view-") {
            rest.strip_suffix(".sock")
        } else if let Some(rest) = name.strip_prefix("stream-") {
            rest.split('-').next()
        } else {
            None
        };
        let Some(pid) = pid.and_then(|pid| pid.parse::<u32>().ok()) else {
            continue;
        };
        if pid == std::process::id() || crate::util::is_pid_alive(pid) {
            continue;
        }
        let path = entry.path();
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

struct Session {
    viewer: Child,
    stream: Option<Child>,
    _files: Option<TempFiles>,
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Some(stream) = &mut self.stream {
            stop_child(stream, Duration::from_secs(2));
        }
        stop_child(&mut self.viewer, Duration::from_secs(1));
    }
}

fn connect_linux(
    peer: &Peer,
    options: &ConnectOptions,
    sender: mpsc::Sender<Event>,
    receiver: Receiver<Event>,
) -> Result<(), String> {
    let name = sanitize(&peer.host);
    println!("checking {name} …");
    let local = if peer.is_self {
        crate::agent::probe()
    } else {
        ssh::probe(&peer.host, options.timeout)
            .map_err(|error| format_connect_error(error, &name))?
    };
    match local.state.as_str() {
        "ready" | "inactive" | "starting" => {}
        "backend-missing" => {
            return Err(format!(
                "{name} has NIRUREMOTE but x11vnc is missing. {}",
                sanitize(&local.backend.install_hint.unwrap_or_default())
            ));
        }
        "no-session" => {
            return Err(format!(
                "{name} has no active X11 desktop session. Log in at the machine first."
            ));
        }
        "unsupported-wayland" => {
            return Err(format!(
                "{name} is running Wayland; NIRUREMOTE supports X11 sessions for now."
            ));
        }
        other => {
            return Err(format!(
                "{name} is not ready: {} ({})",
                sanitize(other),
                sanitize(&local.last_error.unwrap_or_default())
            ));
        }
    }
    if !agent_supports_stream(&local) {
        return Err(format!(
            "{name} runs NIRUREMOTE {}, which only offers the old unauthenticated loopback \
             listener. Update NIRUREMOTE on {name} to {MIN_AGENT} or newer \
             (nirux extras update), then connect again.",
            sanitize(&local.agent_version)
        ));
    }
    if let Some(locker) = &local.locker {
        println!(
            "{name}: the screen is locked ({}). The viewer shows the lock screen; \
             only the account password unlocks it.",
            sanitize(locker)
        );
    }

    let viewer = viewer::find_vnc()
        .ok_or_else(|| format!("no VNC viewer is installed. {}", viewer::vnc_install_hint()))?;

    // A private endpoint for the viewer: one connection, owner-only.
    let directory = crate::util::runtime_dir();
    crate::util::ensure_private_dir(&directory)
        .map_err(|error| format!("runtime directory: {error}"))?;
    remove_stale_runtime(&directory);
    let (listener, endpoint_path, endpoint_port) = if viewer::supports_unix_socket(&viewer) {
        let path = directory.join(format!("view-{}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let socket = std::os::unix::net::UnixListener::bind(&path)
            .map_err(|error| format!("viewer socket {}: {error}", path.display()))?;
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        (LocalListener::Unix(socket, path.clone()), Some(path), None)
    } else {
        let socket = std::net::TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("viewer port: {error}"))?;
        let port = socket
            .local_addr()
            .map_err(|error| format!("viewer port: {error}"))?
            .port();
        (LocalListener::Tcp(socket, port), None, Some(port))
    };
    let endpoint = match (&endpoint_path, endpoint_port) {
        (Some(path), _) => viewer::Endpoint::Socket(path),
        (None, Some(port)) => viewer::Endpoint::Loopback(port),
        (None, None) => unreachable!(),
    };

    println!("opening the viewer …");
    let viewer_child = viewer::spawn_vnc_session(&viewer, endpoint)?;
    events::watch_exit(viewer_child.id(), "viewer", sender.clone());
    let mut session = Session {
        viewer: viewer_child,
        stream: None,
        _files: None,
    };
    accept_one(listener, sender.clone());

    let connection = loop {
        match receiver.recv_timeout(Duration::from_secs(30)) {
            Ok(Event::Accepted(result)) => break result?,
            Ok(Event::Exited("viewer")) => {
                return Err("the viewer closed before it connected".into());
            }
            Ok(Event::Signal) => {
                println!("interrupted");
                return Ok(());
            }
            Ok(_) => {}
            Err(_) => return Err("the viewer did not connect within 30 s".into()),
        }
    };

    // The SSH child's stdin and stdout are the viewer connection itself.
    let files = TempFiles::new("stream")?;
    let stderr = files.open_stderr()?;
    let mut command = if peer.is_self {
        let binary =
            std::env::current_exe().map_err(|error| format!("current executable: {error}"))?;
        let mut command = Command::new(binary);
        command.arg("stream");
        command
    } else {
        ssh::stream_command(&peer.host, options.timeout)
            .map_err(|error| format_connect_error(error, &name))?
    };
    let output = connection
        .try_clone()
        .map_err(|error| format!("viewer connection: {error}"))?;
    command
        .stdin(Stdio::from(connection))
        .stdout(Stdio::from(output))
        .stderr(Stdio::from(stderr));
    configure_child(&mut command, false, true);
    println!("starting the remote desktop on {name} …");
    let stream = command
        .spawn()
        .map_err(|error| format!("could not start SSH: {error}"))?;
    // `command` holds our copies of the connection; drop them so the viewer
    // sees end-of-file as soon as SSH exits.
    drop(command);
    events::watch_exit(stream.id(), "stream", sender);
    session.stream = Some(stream);
    session._files = Some(files);
    println!("connected to {name} · close the viewer window to disconnect");

    loop {
        match receiver.recv() {
            Ok(Event::Exited("viewer")) => {
                drop(session);
                println!("disconnected");
                return Ok(());
            }
            Ok(Event::Exited(_)) => {
                let detail = session
                    ._files
                    .as_ref()
                    .map(TempFiles::read_stderr)
                    .unwrap_or_default();
                drop(session);
                let detail = sanitize(detail.trim());
                let detail = detail
                    .strip_prefix("niruremote: ")
                    .unwrap_or(&detail)
                    .to_string();
                return if detail.is_empty() {
                    println!("connection to {name} closed");
                    Ok(())
                } else {
                    Err(format!("connection to {name} ended: {detail}"))
                };
            }
            Ok(Event::Signal) => {
                drop(session);
                println!("interrupted");
                return Ok(());
            }
            Ok(_) => {}
            Err(_) => return Ok(()),
        }
    }
}

fn connect_windows(
    peer: &Peer,
    sender: mpsc::Sender<Event>,
    receiver: Receiver<Event>,
) -> Result<(), String> {
    let target = sanitize(peer.ip.as_deref().unwrap_or(&peer.host));
    if !port_open_ip(&target, 3389, Duration::from_secs(2)) {
        return Err(format!(
            "RDP port 3389 is not reachable on {target}; enable Remote Desktop on Windows"
        ));
    }
    let viewer = viewer::find_rdp()
        .ok_or_else(|| format!("no RDP client is installed. {}", viewer::rdp_install_hint()))?;
    println!("connecting to {target} over RDP");
    let child = viewer::spawn_rdp(&viewer, &target)?;
    wait_for_viewer(child, sender, receiver)
}

fn connect_macos(
    peer: &Peer,
    sender: mpsc::Sender<Event>,
    receiver: Receiver<Event>,
) -> Result<(), String> {
    let target = sanitize(peer.ip.as_deref().unwrap_or(&peer.host));
    if !port_open_ip(&target, 5900, Duration::from_secs(2)) {
        return Err(format!(
            "Screen Sharing port 5900 is not reachable on {target}; enable Screen Sharing on macOS"
        ));
    }
    let viewer = viewer::find_vnc()
        .ok_or_else(|| format!("no VNC viewer is installed. {}", viewer::vnc_install_hint()))?;
    println!("connecting to {target} over VNC");
    let child = viewer::spawn_vnc_host(&viewer, &target, 5900)?;
    wait_for_viewer(child, sender, receiver)
}

/// Block until the viewer exits or a signal arrives. No polling.
fn wait_for_viewer(
    mut child: Child,
    sender: mpsc::Sender<Event>,
    receiver: Receiver<Event>,
) -> Result<(), String> {
    events::watch_exit(child.id(), "viewer", sender);
    loop {
        match receiver.recv() {
            Ok(Event::Exited(_)) => {
                let _ = child.wait();
                println!("disconnected");
                return Ok(());
            }
            Ok(Event::Signal) => {
                stop_child(&mut child, Duration::from_secs(1));
                println!("interrupted");
                return Ok(());
            }
            Ok(_) => {}
            Err(_) => {
                stop_child(&mut child, Duration::from_secs(1));
                return Ok(());
            }
        }
    }
}

fn format_connect_error(error: SshError, host: &str) -> String {
    let detail = sanitize(&error.detail);
    match error.state.as_str() {
        "not-installed" => {
            format!("{detail} Install NIRUREMOTE on the remote host, then try again.")
        }
        "backend-missing" => {
            format!("{detail} Install x11vnc on the remote host (niruremote setup --run).")
        }
        "host-key-untrusted" => {
            format!("{detail} Run `ssh {host}` once in a terminal and verify the fingerprint.")
        }
        "ssh-denied" => {
            format!("{detail} Load the SSH key or run `ssh {host}` once in a terminal.")
        }
        _ => detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::LocalState;

    fn agent(version: &str, state: &str, modes: &[&str]) -> LocalState {
        let mut local = LocalState::new();
        local.agent_version = version.into();
        local.state = state.into();
        local.supported_modes = modes.iter().map(|mode| mode.to_string()).collect();
        local.session.display = Some(":0".into());
        local
    }

    #[test]
    fn old_agents_are_reported_as_update_needed() {
        let cached = map_linux(agent("0.1.6", "inactive", &["ssh-tunnel"]), 0);
        assert_eq!(cached.state, "update-needed");
        let cached = map_linux(agent("0.1.6", "ready", &["ssh-tunnel"]), 0);
        assert_eq!(cached.state, "update-needed");
    }

    #[test]
    fn stream_agents_are_ready_without_enable() {
        let cached = map_linux(agent("0.2.0", "inactive", &[MODE_STREAM]), 0);
        assert_eq!(cached.state, "ready");
        assert!(cached.detail.contains("connect any time"));
        let mut locked = agent("0.2.0", "inactive", &[MODE_STREAM]);
        locked.locker = Some("nirulock".into());
        assert!(map_linux(locked, 0).detail.contains("screen locked"));
    }

    #[test]
    fn hostile_agent_strings_are_sanitized() {
        let mut local = agent("0.2.0\u{1b}[2J", "weird\u{7}", &[MODE_STREAM]);
        local.session.display = Some(":0\u{1b}]0;x".into());
        let cached = map_linux(local, 0);
        assert!(!cached.detail.chars().any(char::is_control));
        assert!(!cached.version.unwrap().chars().any(char::is_control));
    }

    #[test]
    fn versions_compare_numerically() {
        assert!(version_tuple("0.10.0") > version_tuple("0.2.0"));
        assert!(version_tuple("0.2.0") >= version_tuple(MIN_AGENT));
        assert!(version_tuple("0.1.6") < version_tuple(MIN_AGENT));
    }
}

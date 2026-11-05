use crate::events::{self, Event};
use crate::json::{BackendInfo, EnsureResponse, LocalState, MODE_STREAM};
use crate::state::{self, StateLock};
use crate::util::{
    create_private_file, ensure_private_dir, find_in_path, free_loopback_port, human_age_opt,
    is_pid_alive, now_secs, process_matches, read_tail, runtime_dir, sanitize, tcp_listening_local,
    terminate_pid,
};
use crate::{session, viewer, x11vnc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::os::fd::FromRawFd;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub fn probe() -> LocalState {
    let session = session::discover();
    let backend = backend_info();
    let runtime = state::valid_runtime();

    let mut local = LocalState::new();
    local.hostname = crate::util::hostname();
    local.user = crate::util::username();
    local.os = std::env::consts::OS.into();
    local.session = session;
    local.backend = backend;
    local.locker = session::screen_locker();
    local.sessions = live_sessions().len() as u32;
    if local.session.session_type == "x11" {
        local.supported_modes.push(MODE_STREAM.into());
    }

    if let Some(runtime) = runtime {
        local.enabled = true;
        local.port = Some(runtime.port);
        local.pid = Some(runtime.pid);
        local.since = Some(runtime.started_at);
        local.owner = Some(runtime.owner);
        local.running = tcp_listening_local(runtime.port);
        local.supported_modes.push("ssh-tunnel".into());
        if local.running {
            local.state = "ready".into();
        } else {
            local.state = "starting".into();
            local.last_error =
                Some("x11vnc is running but the loopback port is not open yet".into());
        }
        return local;
    }

    if local.session.session_type == "wayland" {
        local.state = "unsupported-wayland".into();
        local.last_error = Some("NIRUREMOTE currently attaches to X11 sessions only".into());
    } else if local.session.session_type != "x11" || local.session.display.is_none() {
        local.state = "no-session".into();
        local.last_error = Some("no active X11 desktop session was found".into());
    } else if !local.backend.installed {
        local.state = "backend-missing".into();
        local.last_error = Some("x11vnc is not installed".into());
    } else {
        local.state = "inactive".into();
    }
    local
}

fn backend_info() -> BackendInfo {
    match x11vnc::binary() {
        Ok(path) => {
            let version = x11vnc::version(&path);
            BackendInfo {
                name: "x11vnc".into(),
                installed: true,
                path: Some(path.display().to_string()),
                version,
                install_hint: Some(x11vnc::install_hint()),
            }
        }
        Err(_) => BackendInfo {
            name: "x11vnc".into(),
            installed: false,
            path: None,
            version: None,
            install_hint: Some(x11vnc::install_hint()),
        },
    }
}

pub fn print_local(local: &LocalState, json: bool) -> Result<(), String> {
    if json {
        let text = serde_json::to_string_pretty(local).map_err(|error| error.to_string())?;
        println!("{text}");
        return Ok(());
    }
    println!("host:      {} ({})", local.hostname, local.user);
    println!("agent:     {} schema {}", local.agent_version, local.schema);
    println!(
        "session:   {}{} ({})",
        local.session.session_type,
        local
            .session
            .display
            .as_deref()
            .map(|display| format!(" {display}"))
            .unwrap_or_default(),
        local.session.source
    );
    if let Some(auth) = &local.session.auth_file {
        println!("xauth:     {auth}");
    }
    if let Some(locker) = &local.locker {
        println!("screen:    locked ({locker})");
    }
    println!(
        "x11vnc:    {}{}",
        if local.backend.installed {
            "installed"
        } else {
            "missing"
        },
        local
            .backend
            .version
            .as_deref()
            .map(|version| format!(" ({version})"))
            .unwrap_or_default()
    );
    if !local.backend.installed {
        if let Some(hint) = &local.backend.install_hint {
            println!("install:   {hint}");
        }
    }
    println!("state:     {}", local.state);
    println!(
        "listener:  {}",
        local
            .port
            .map(|port| format!("127.0.0.1:{port} (password, enabled)"))
            .unwrap_or_else(|| "none".into())
    );
    println!("sessions:  {}", local.sessions);
    if let Some(pid) = local.pid {
        println!("pid:       {pid}");
    }
    if let Some(owner) = &local.owner {
        println!("owner:     {owner}");
    }
    if local.since.is_some() {
        println!("since:     {}", human_age_opt(local.since));
    }
    println!(
        "modes:     {}",
        if local.supported_modes.is_empty() {
            "none".into()
        } else {
            local.supported_modes.join(", ")
        }
    );
    if let Some(error) = &local.last_error {
        println!("detail:    {error}");
    }
    Ok(())
}

pub fn enable() -> Result<(), String> {
    let _lock = StateLock::acquire()?;
    let local = wait_for_ready(probe(), Duration::from_secs(2));
    if local.state == "starting" {
        return Err("x11vnc is still starting; try again in a moment".into());
    }
    if local.running {
        println!(
            "already enabled on 127.0.0.1:{} (pid {})",
            local.port.unwrap_or(0),
            local.pid.unwrap_or(0)
        );
        println!(
            "The password was shown when it was enabled. For a new one: \
             niruremote disable && niruremote enable"
        );
        return Ok(());
    }
    validate_start(&local)?;

    let port = free_loopback_port()?;
    let directory = runtime_dir();
    ensure_private_dir(&directory).map_err(|error| format!("runtime directory: {error}"))?;
    let password_file = directory.join(format!("enable-{}.pw", std::process::id()));
    let _ = fs::remove_file(&password_file);
    // VNC authentication uses at most 8 characters.
    let password = crate::util::random_secret(8)?;
    {
        let mut file = create_private_file(&password_file)
            .map_err(|error| format!("password file: {error}"))?;
        writeln!(file, "{password}").map_err(|error| format!("password file: {error}"))?;
    }
    let started = x11vnc::start_listener(&local.session, port, &password_file, &state::log_path());
    // x11vnc removes the file itself after reading it (`rm:`); make sure.
    let _ = fs::remove_file(&password_file);
    let pid = started?;

    let runtime = state::new_runtime(
        pid,
        port,
        local.session.display.clone().unwrap_or_default(),
        local.session.auth_file.clone(),
        "x11vnc".to_string(),
        "enable",
    );
    if let Err(error) = state::write_runtime(&runtime) {
        x11vnc::stop(pid);
        return Err(error);
    }
    println!("enabled a VNC listener on 127.0.0.1:{port} (pid {pid})");
    println!("password:  {password}");
    println!();
    println!("The password is shown only now. The listener is loopback-only and stays");
    println!("until `niruremote disable`, logout or reboot. It is meant for manual");
    println!("tunnels from Windows/macOS clients:");
    println!(
        "    ssh -N -L 5900:127.0.0.1:{port} {}@{}",
        crate::util::username(),
        crate::util::hostname()
    );
    println!("`niruremote connect` from a Linux client does not need `enable`.");
    Ok(())
}

pub fn disable() -> Result<(), String> {
    let _lock = StateLock::acquire()?;
    let mut stopped_anything = false;
    if let Some(runtime) = state::read_runtime() {
        if process_matches(runtime.pid, "x11vnc") {
            x11vnc::stop(runtime.pid);
            println!("stopped the VNC listener (pid {})", runtime.pid);
        } else if is_pid_alive(runtime.pid) {
            println!(
                "removed stale state; pid {} is not x11vnc, so it was not killed",
                runtime.pid
            );
        } else {
            println!("removed stale listener state");
        }
        state::remove_runtime();
        stopped_anything = true;
    }
    for record in live_sessions() {
        terminate_pid(record.pid, Duration::from_secs(3));
        println!("ended remote session (pid {})", record.pid);
        stopped_anything = true;
    }
    if !stopped_anything {
        println!("remote desktop is not running");
    }
    Ok(())
}

/// `ensure` served 0.1.x clients, which forwarded an unauthenticated
/// loopback port. 0.2.0 clients use `stream`; refuse clearly instead.
pub fn ensure(json: bool) -> Result<(), String> {
    let message = format!(
        "this host runs NIRUREMOTE {}; update NIRUREMOTE on the client to 0.2.0 or newer \
         (nirux extras update), then connect again",
        env!("CARGO_PKG_VERSION")
    );
    if json {
        let mut local = probe();
        local.state = "upgrade-required".into();
        local.last_error = Some(message.clone());
        let response = EnsureResponse {
            local,
            owned: false,
            log: None,
        };
        let text = serde_json::to_string(&response).map_err(|error| error.to_string())?;
        println!("{text}");
        let _ = std::io::stdout().flush();
    }
    Err(message)
}

fn wait_for_ready(mut local: LocalState, timeout: Duration) -> LocalState {
    if local.state != "starting" {
        return local;
    }
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
        local = probe();
        if local.state != "starting" {
            break;
        }
    }
    local
}

fn validate_start(local: &LocalState) -> Result<(), String> {
    if local.session.session_type != "x11" || local.session.display.is_none() {
        return Err(local
            .last_error
            .clone()
            .unwrap_or_else(|| "no active X11 desktop session".into()));
    }
    if !local.backend.installed {
        return Err(format!(
            "x11vnc is not installed. {}",
            local.backend.install_hint.clone().unwrap_or_default()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// `stream`: one remote desktop session over the SSH channel's stdin/stdout.

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionRecord {
    pub pid: u32,
    pub x11vnc_pid: u32,
    pub started_at: u64,
    pub from: String,
}

fn session_path(pid: u32) -> PathBuf {
    runtime_dir().join(format!("session-{pid}.json"))
}

/// Live `stream` sessions; stale records are removed.
pub fn live_sessions() -> Vec<SessionRecord> {
    let mut sessions = Vec::new();
    let Ok(entries) = fs::read_dir(runtime_dir()) else {
        return sessions;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_record = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("session-") && name.ends_with(".json"));
        if !is_record {
            continue;
        }
        match crate::util::read_json::<SessionRecord>(&path) {
            Some(record) if process_matches(record.pid, "niruremote") => sessions.push(record),
            _ => {
                let _ = fs::remove_file(&path);
            }
        }
    }
    sessions
}

fn idle_limit() -> Duration {
    let seconds = std::env::var("NIRUREMOTE_IDLE_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|seconds| (2..=600).contains(seconds))
        .unwrap_or(30);
    Duration::from_secs(seconds)
}

fn ssh_source() -> String {
    std::env::var("SSH_CLIENT")
        .ok()
        .and_then(|value| value.split_whitespace().next().map(sanitize))
        .unwrap_or_else(|| "this machine".into())
}

/// Best-effort desktop notification so the person at the machine sees that a
/// remote session started or ended. Bounded; failures are ignored.
fn notify(body: &str) {
    if std::env::var("NIRUREMOTE_NOTIFY").is_ok_and(|value| value == "off") {
        return;
    }
    let Some(binary) = find_in_path("notify-send") else {
        return;
    };
    let bus = std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().or_else(|| {
        let path = format!("/run/user/{}/bus", crate::util::uid());
        Path::new(&path)
            .exists()
            .then(|| format!("unix:path={path}"))
    });
    let Some(bus) = bus else {
        return;
    };
    let binary = binary.display().to_string();
    let _ = crate::util::run_capped_env(
        &binary,
        &[
            "-a",
            "NIRUREMOTE",
            "-i",
            "preferences-desktop-remote-desktop",
            "Remote desktop",
            body,
        ],
        &[("DBUS_SESSION_BUS_ADDRESS", bus.as_str())],
        Duration::from_secs(2),
        4096,
    );
}

fn log_line(message: &str) {
    if let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(state::log_path())
    {
        let _ = writeln!(file, "niruremote {}: {message}", now_secs());
    }
}

pub fn stream() -> Result<(), String> {
    let local = probe();
    validate_start(&local)?;
    let source = ssh_source();

    let directory = runtime_dir();
    ensure_private_dir(&directory).map_err(|error| format!("runtime directory: {error}"))?;
    let (ours, theirs) = UnixStream::pair().map_err(|error| format!("socketpair: {error}"))?;
    let mut child = x11vnc::spawn_stream(&local.session, theirs, &state::log_path())?;
    let x11vnc_pid = child.id();

    let record = SessionRecord {
        pid: std::process::id(),
        x11vnc_pid,
        started_at: now_secs(),
        from: source.clone(),
    };
    let record_path = session_path(record.pid);
    let _ = crate::util::write_json(&record_path, &record);
    log_line(&format!(
        "session started from {source} (x11vnc pid {x11vnc_pid})"
    ));
    {
        let body = format!("Session started from {source}");
        std::thread::spawn(move || notify(&body));
    }

    let (sender, receiver) = mpsc::channel();
    events::watch_exit(x11vnc_pid, "x11vnc", sender.clone());
    events::watch_signals(sender.clone())?;

    let started = Instant::now();
    let last_input = Arc::new(AtomicU64::new(0));

    // Viewer -> x11vnc. Every byte from the viewer is proof the path is alive.
    {
        let mut to_x11vnc = ours
            .try_clone()
            .map_err(|error| format!("stream: {error}"))?;
        let last_input = Arc::clone(&last_input);
        let sender = sender.clone();
        std::thread::spawn(move || {
            // fd 0 is the SSH channel; nothing else in `stream` reads it.
            let mut input = unsafe { fs::File::from_raw_fd(0) };
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                match input.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        last_input.store(started.elapsed().as_millis() as u64, Ordering::Relaxed);
                        if to_x11vnc.write_all(&buffer[..count]).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let _ = to_x11vnc.shutdown(std::net::Shutdown::Write);
            let _ = sender.send(Event::Closed("viewer"));
        });
    }
    // x11vnc -> viewer.
    {
        let mut from_x11vnc = ours
            .try_clone()
            .map_err(|error| format!("stream: {error}"))?;
        let sender = sender.clone();
        std::thread::spawn(move || {
            // fd 1 carries only RFB bytes; `stream` never prints.
            let mut output = unsafe { fs::File::from_raw_fd(1) };
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                match from_x11vnc.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        if output.write_all(&buffer[..count]).is_err() {
                            break;
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
            let _ = sender.send(Event::Closed("x11vnc"));
        });
    }
    drop(ours);
    drop(sender);

    // Block until something ends the session. The only timed wake-up is the
    // dead-path watchdog, scheduled for when the idle limit could expire.
    let limit = idle_limit();
    let reason: Option<String> = loop {
        let idle = started
            .elapsed()
            .saturating_sub(Duration::from_millis(last_input.load(Ordering::Relaxed)));
        if idle >= limit {
            break Some(format!(
                "no traffic from the viewer for {} s; the connection was lost",
                limit.as_secs()
            ));
        }
        match receiver.recv_timeout(limit - idle) {
            Ok(Event::Closed("viewer")) => break None,
            Ok(Event::Closed(_)) | Ok(Event::Exited(_)) => {
                break Some("x11vnc stopped".into());
            }
            Ok(Event::Signal) => break Some("stopped by a signal".into()),
            Ok(Event::Accepted(_)) => {}
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break None,
        }
    };

    crate::util::stop_child(&mut child, Duration::from_secs(2));
    let _ = fs::remove_file(&record_path);
    log_line(&format!(
        "session from {source} ended: {}",
        reason.as_deref().unwrap_or("viewer disconnected")
    ));
    notify(&format!("Session from {source} ended"));
    match reason {
        None => Ok(()),
        Some(reason) => Err(format!("remote session ended: {reason}")),
    }
}

pub fn setup(run: bool) -> Result<(), String> {
    let local = probe();
    println!("niruremote {}", local.agent_version);
    println!("host:      {} ({})", local.hostname, local.user);
    println!(
        "session:   {}{} ({})",
        local.session.session_type,
        local
            .session
            .display
            .as_deref()
            .map(|display| format!(" {display}"))
            .unwrap_or_default(),
        local.session.source
    );
    println!(
        "x11vnc:    {}",
        local.backend.path.as_deref().unwrap_or("missing")
    );
    match viewer::find_vnc() {
        Some(_) => println!("vnc viewer: installed"),
        None => println!("vnc viewer: missing"),
    }
    match viewer::find_rdp() {
        Some(_) => println!("rdp client: installed"),
        None => println!("rdp client: missing (only needed for Windows hosts)"),
    }
    println!(
        "tailscale:  {}",
        find_in_path("tailscale")
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "missing".into())
    );
    println!(
        "ssh:        {}",
        find_in_path("ssh")
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "missing".into())
    );
    println!("state:      {}", local.state);

    let missing_backend = !local.backend.installed;
    let missing_viewer = viewer::find_vnc().is_none();
    if !missing_backend && !missing_viewer {
        println!("All local packages needed for a Linux desktop host are installed.");
        return Ok(());
    }

    let Some(mut plan) = package_plan() else {
        if missing_backend {
            println!("Install x11vnc with your package manager.");
        }
        if missing_viewer {
            println!("{}", viewer::vnc_install_hint());
        }
        return Ok(());
    };
    adjust_privilege(&mut plan)?;
    println!("\nSuggested package command:");
    println!("  {}", plan.join(" "));
    if run {
        println!("Running it now; sudo may ask for your password.");
        let status = Command::new(&plan[0])
            .args(&plan[1..])
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .map_err(|error| format!("could not run {}: {error}", plan[0]))?;
        if !status.success() {
            return Err(format!("package command failed with {status}"));
        }
        println!("Package installation finished. Re-run `niruremote setup` to verify.");
    }
    Ok(())
}

fn adjust_privilege(plan: &mut Vec<String>) -> Result<(), String> {
    if crate::util::uid() == 0 {
        if plan
            .first()
            .is_some_and(|command| command == "sudo" || command == "doas")
        {
            plan.remove(0);
        }
        return Ok(());
    }
    if plan.first().is_some_and(|command| command == "sudo")
        && find_in_path("sudo").is_none()
        && find_in_path("doas").is_some()
    {
        plan[0] = "doas".into();
    }
    Ok(())
}

fn package_plan() -> Option<Vec<String>> {
    let release = std::fs::read_to_string("/etc/os-release").ok()?;
    let mut id = String::new();
    let mut like = String::new();
    for line in release.lines() {
        if let Some(value) = line.strip_prefix("ID=") {
            id = value.trim_matches('"').to_lowercase();
        } else if let Some(value) = line.strip_prefix("ID_LIKE=") {
            like = value.trim_matches('"').to_lowercase();
        }
    }
    let family = format!("{id} {like}");
    package_plan_for(&family)
}

fn package_plan_for(family: &str) -> Option<Vec<String>> {
    let args: Vec<&str> = if family.contains("arch") {
        vec![
            "sudo",
            "pacman",
            "-S",
            "--needed",
            "--noconfirm",
            "x11vnc",
            "tigervnc",
        ]
    } else if family.contains("void") {
        vec!["sudo", "xbps-install", "-Sy", "x11vnc", "tigervnc"]
    } else if family.contains("fedora") || family.contains("rhel") {
        vec!["sudo", "dnf", "install", "-y", "x11vnc", "tigervnc"]
    } else if family.contains("suse") {
        vec!["sudo", "zypper", "install", "-y", "x11vnc", "tigervnc"]
    } else if family.contains("debian") || family.contains("ubuntu") {
        vec![
            "sudo",
            "apt-get",
            "install",
            "-y",
            "x11vnc",
            "tigervnc-viewer",
        ]
    } else {
        return None;
    };
    Some(args.into_iter().map(str::to_string).collect())
}

pub fn doctor() -> Result<(), String> {
    let local = probe();
    println!("niruremote {}", local.agent_version);
    println!("os:         {}", std::env::consts::OS);
    println!("host:       {}", local.hostname);
    println!("user:       {}", local.user);
    println!("state dir:  {}", crate::util::state_dir().display());
    println!("cache dir:  {}", crate::util::cache_dir().display());
    println!("log:        {}", state::log_path().display());
    println!("state:      {}", local.state);
    println!(
        "session:    {} {} ({})",
        local.session.session_type,
        local.session.display.as_deref().unwrap_or("-"),
        local.session.source
    );
    println!(
        "xauth:      {}",
        local.session.auth_file.as_deref().unwrap_or("-")
    );
    println!(
        "x11vnc:     {}{}",
        local.backend.path.as_deref().unwrap_or("-"),
        local
            .backend
            .version
            .as_deref()
            .map(|version| format!(" ({version})"))
            .unwrap_or_default()
    );
    println!(
        "vnc viewer: {}",
        viewer::find_vnc()
            .map(|viewer| format!("{viewer:?}"))
            .unwrap_or_else(|| "-".into())
    );
    println!(
        "rdp client: {}",
        viewer::find_rdp()
            .map(|viewer| format!("{viewer:?}"))
            .unwrap_or_else(|| "-".into())
    );
    println!(
        "tailscale:  {}",
        find_in_path("tailscale")
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".into())
    );
    println!(
        "ssh:        {}",
        find_in_path("ssh")
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".into())
    );
    println!("loginctl:   {}", find_in_path("loginctl").is_some());
    println!("who:        {}", find_in_path("who").is_some());
    if let Some(tail) = read_tail(&state::log_path(), 4096).filter(|tail| !tail.is_empty()) {
        println!("\nlast x11vnc log:\n{tail}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_plans_cover_the_required_platforms() {
        let debian = package_plan_for("ubuntu debian").unwrap();
        assert_eq!(debian[1], "apt-get");
        assert!(debian.iter().any(|argument| argument == "tigervnc-viewer"));

        let arch = package_plan_for("arch").unwrap();
        assert_eq!(arch[1], "pacman");
        assert!(arch.iter().any(|argument| argument == "tigervnc"));

        let void = package_plan_for("void").unwrap();
        assert_eq!(void[1], "xbps-install");
        assert!(void.iter().any(|argument| argument == "tigervnc"));

        assert!(package_plan_for("plan9").is_none());
    }
}

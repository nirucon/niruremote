use crate::json::{BackendInfo, EnsureResponse, LocalState};
use crate::state::{self, StateLock};
use crate::util::{find_in_path, free_loopback_port, human_age_opt, port_open_local, read_tail};
use crate::{session, viewer, x11vnc};
use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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
    local.supported_modes = if local.session.session_type == "x11" {
        vec!["ssh-tunnel".into()]
    } else {
        Vec::new()
    };

    if let Some(runtime) = runtime {
        local.enabled = true;
        local.port = Some(runtime.port);
        local.pid = Some(runtime.pid);
        local.since = Some(runtime.started_at);
        local.owner = Some(runtime.owner);
        local.running = port_open_local(runtime.port, Duration::from_millis(200));
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
    if local.session.auth_file.is_some() {
        println!(
            "xauth:     {}",
            local.session.auth_file.as_deref().unwrap_or_default()
        );
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
    if !local.backend.installed
        && let Some(hint) = &local.backend.install_hint
    {
        println!("install:   {hint}");
    }
    println!("state:     {}", local.state);
    println!(
        "loopback:  {}",
        local
            .port
            .map(|port| format!("127.0.0.1:{port}"))
            .unwrap_or_else(|| "not listening".into())
    );
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
        return Ok(());
    }
    validate_start(&local)?;
    let (started_local, _child) = start_new(&local, x11vnc::StartMode::Detached, "enable")?;
    println!(
        "enabled remote desktop on 127.0.0.1:{} (pid {})",
        started_local.port.unwrap_or(0),
        started_local.pid.unwrap_or(0)
    );
    println!("It stays available until `niruremote disable`, logout or reboot.");
    Ok(())
}

pub fn disable() -> Result<(), String> {
    let _lock = StateLock::acquire()?;
    let Some(runtime) = state::read_runtime() else {
        println!("remote desktop is not running");
        return Ok(());
    };
    if crate::util::process_matches(runtime.pid, "x11vnc") {
        x11vnc::stop(runtime.pid);
        println!("disabled remote desktop (pid {})", runtime.pid);
    } else if crate::util::is_pid_alive(runtime.pid) {
        println!(
            "removed stale state; pid {} is not x11vnc, so it was not killed",
            runtime.pid
        );
    } else {
        println!("removed stale remote desktop state");
    }
    state::remove_runtime();
    Ok(())
}

pub fn ensure(hold: bool, json: bool) -> Result<(), String> {
    if hold {
        let (response, child) = {
            let _lock = StateLock::acquire()?;
            let local = wait_for_ready(probe(), Duration::from_secs(2));
            if local.state == "starting" {
                return Err("x11vnc is still starting; try again in a moment".into());
            }
            if local.running {
                let response = EnsureResponse {
                    local,
                    owned: false,
                    log: Some(state::log_path().display().to_string()),
                };
                print_ensure(&response, json)?;
                return Ok(());
            }
            validate_start(&local)?;
            let (started_local, child) = start_new(&local, x11vnc::StartMode::Held, "ensure")?;
            let response = EnsureResponse {
                local: started_local,
                owned: true,
                log: Some(state::log_path().display().to_string()),
            };
            print_ensure(&response, json)?;
            (response, child)
        };

        let Some(child) = child else {
            return Ok(());
        };
        let pid = response.local.pid.unwrap_or(0);
        wait_held(child, pid);
        return Ok(());
    }

    let _lock = StateLock::acquire()?;
    let local = wait_for_ready(probe(), Duration::from_secs(2));
    if local.state == "starting" {
        return Err("x11vnc is still starting; try again in a moment".into());
    }
    let (local, owned) = if local.running {
        (local, false)
    } else {
        validate_start(&local)?;
        let (started, _child) = start_new(&local, x11vnc::StartMode::Detached, "ensure")?;
        (started, true)
    };
    let response = EnsureResponse {
        local,
        owned,
        log: Some(state::log_path().display().to_string()),
    };
    print_ensure(&response, json)?;
    Ok(())
}

fn print_ensure(response: &EnsureResponse, json: bool) -> Result<(), String> {
    if json {
        let text = serde_json::to_string(response).map_err(|error| error.to_string())?;
        println!("{text}");
        std::io::stdout()
            .flush()
            .map_err(|error| format!("stdout: {error}"))?;
    } else {
        print_local(&response.local, false)?;
        println!("owned:     {}", response.owned);
    }
    Ok(())
}

fn wait_for_ready(mut local: LocalState, timeout: Duration) -> LocalState {
    if local.state != "starting" {
        return local;
    }
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
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

fn start_new(
    local: &LocalState,
    mode: x11vnc::StartMode,
    owner: &str,
) -> Result<(LocalState, Option<Child>), String> {
    let port = free_loopback_port()?;
    let started = x11vnc::start(&local.session, port, mode, &state::log_path())?;
    let display = local.session.display.clone().unwrap_or_default();
    let runtime = state::new_runtime(
        started.pid,
        port,
        display,
        local.session.auth_file.clone(),
        "x11vnc".to_string(),
        owner,
    );
    if let Err(error) = state::write_runtime(&runtime) {
        x11vnc::stop(started.pid);
        return Err(error);
    }
    let mut started_local = probe();
    if started_local.state != "ready" {
        started_local.state = "ready".into();
        started_local.running = true;
        started_local.last_error = None;
    }
    Ok((started_local, started.child))
}

fn wait_held(mut child: Child, pid: u32) {
    let stop = Arc::new(AtomicBool::new(false));
    let reader_stop = Arc::clone(&stop);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut handle = stdin.lock();
        let mut buffer = [0u8; 256];
        loop {
            match handle.read(&mut buffer) {
                Ok(0) => break,
                Ok(_) => {}
                Err(_) => break,
            }
        }
        reader_stop.store(true, Ordering::SeqCst);
    });

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(_) => break,
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    x11vnc::stop(pid);
    state::remove_runtime();
    if !stop.load(Ordering::SeqCst) {
        eprintln!("niruremote: x11vnc stopped");
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
    if let Some(tail) = read_tail(&state::log_path(), 4096)
        && !tail.is_empty()
    {
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

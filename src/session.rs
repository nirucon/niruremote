use crate::json::SessionInfo;
use crate::util::{home_dir, run_capped, uid, username};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub fn discover() -> SessionInfo {
    if let Some(display) = environment_display() {
        return SessionInfo {
            session_type: "x11".into(),
            display: Some(display),
            active: true,
            source: "environment".into(),
            auth_file: find_auth(),
        };
    }

    if let Some(session) = from_logind() {
        return session;
    }

    if let Some(session) = from_who() {
        return session;
    }

    if let Some(session) = from_proc() {
        return session;
    }

    SessionInfo {
        session_type: "none".into(),
        display: None,
        active: false,
        source: "none".into(),
        auth_file: find_auth(),
    }
}

fn environment_display() -> Option<String> {
    if let Ok(display) = std::env::var("NIRUREMOTE_DISPLAY")
        && display.starts_with(':')
    {
        return Some(display);
    }
    let display = std::env::var("DISPLAY").ok()?;
    if !display.starts_with(':') {
        return None;
    }
    if x_socket_exists(&display) {
        Some(display)
    } else {
        None
    }
}

fn x_socket_exists(display: &str) -> bool {
    let number = display
        .trim_start_matches(':')
        .split('.')
        .next()
        .unwrap_or_default();
    if number.is_empty() {
        return false;
    }
    PathBuf::from(format!("/tmp/.X11-unix/X{number}")).exists()
}

fn from_logind() -> Option<SessionInfo> {
    let sessions = run_capped(
        "loginctl",
        &["list-sessions", "--no-legend"],
        Duration::from_secs(2),
        128 * 1024,
    )
    .ok()?;
    if !sessions.success() {
        return None;
    }

    let auth = find_auth();
    let user = username();
    let mut fallback: Option<SessionInfo> = None;
    for line in String::from_utf8_lossy(&sessions.stdout).lines() {
        let Some(id) = line.split_whitespace().next() else {
            continue;
        };
        if id.is_empty() {
            continue;
        }
        let Some(properties) = show_session(id) else {
            continue;
        };
        if properties
            .get("User")
            .is_some_and(|value| value != &uid().to_string() && value != &user)
        {
            continue;
        }
        let Some(display) = properties.get("Display").cloned() else {
            continue;
        };
        let active = properties.get("Active").is_some_and(|value| value == "yes")
            || properties
                .get("State")
                .is_some_and(|value| value == "active");
        let session = if display.starts_with(':') {
            SessionInfo {
                session_type: "x11".into(),
                display: Some(display),
                active,
                source: "logind".into(),
                auth_file: auth.clone(),
            }
        } else if display.starts_with("wayland") {
            SessionInfo {
                session_type: "wayland".into(),
                display: Some(display),
                active,
                source: "logind".into(),
                auth_file: None,
            }
        } else {
            continue;
        };
        if active {
            return Some(session);
        }
        fallback.get_or_insert(session);
    }
    fallback
}

fn show_session(id: &str) -> Option<HashMap<String, String>> {
    let output = run_capped(
        "loginctl",
        &[
            "show-session",
            id,
            "-p",
            "Type",
            "-p",
            "Display",
            "-p",
            "Active",
            "-p",
            "State",
            "-p",
            "User",
        ],
        Duration::from_secs(2),
        64 * 1024,
    )
    .ok()?;
    if !output.success() {
        return None;
    }
    Some(parse_properties(&output.stdout_text()))
}

fn parse_properties(text: &str) -> HashMap<String, String> {
    let mut properties = HashMap::new();
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        properties.insert(key.trim().to_string(), value.trim().to_string());
    }
    properties
}

fn from_who() -> Option<SessionInfo> {
    let output = run_capped("who", &[], Duration::from_secs(2), 128 * 1024).ok()?;
    if !output.success() {
        return None;
    }
    let user = username();
    for line in output.stdout_text().lines() {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 2 {
            continue;
        }
        if fields[0] != user {
            continue;
        }
        let display = fields[1];
        if display.starts_with(':') && x_socket_exists(display) {
            return Some(SessionInfo {
                session_type: "x11".into(),
                display: Some(display.to_string()),
                active: true,
                source: "who".into(),
                auth_file: find_auth(),
            });
        }
    }
    None
}

fn from_proc() -> Option<SessionInfo> {
    let our_uid = uid();
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let name = entry.file_name();
        let Some(pid) = name.to_str() else {
            continue;
        };
        if pid.parse::<u32>().is_err() {
            continue;
        }
        let Ok(status) = std::fs::read_to_string(entry.path().join("status")) else {
            continue;
        };
        let owner = status
            .lines()
            .find_map(|line| line.strip_prefix("Uid:"))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|value| value.parse::<u32>().ok());
        if owner != Some(our_uid) {
            continue;
        }
        let Ok(cmdline) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let args: Vec<String> = cmdline
            .split(|byte| *byte == 0)
            .filter(|part| !part.is_empty())
            .map(|part| String::from_utf8_lossy(part).into_owned())
            .collect();
        let Some(program) = args.first() else {
            continue;
        };
        let base = std::path::Path::new(program)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        if !matches!(
            base,
            "Xorg" | "X" | "Xvfb" | "Xvnc" | "Xtightvnc" | "Xtigervnc"
        ) {
            continue;
        }
        if let Some((display, auth_file)) = x_display_from_args(&args) {
            return Some(SessionInfo {
                session_type: "x11".into(),
                display: Some(display),
                active: true,
                source: "proc".into(),
                auth_file,
            });
        }
    }
    None
}

fn x_display_from_args(args: &[String]) -> Option<(String, Option<String>)> {
    let display = args
        .iter()
        .skip(1)
        .find(|argument| argument.starts_with(':'))
        .cloned()
        .or_else(|| {
            args.iter()
                .position(|argument| argument == "-display")
                .and_then(|index| args.get(index + 1))
                .cloned()
        })?;
    if !display.starts_with(':') {
        return None;
    }
    let auth_file = args
        .iter()
        .position(|argument| argument == "-auth")
        .and_then(|index| args.get(index + 1))
        .filter(|path| PathBuf::from(path).exists())
        .cloned()
        .or_else(find_auth);
    Some((display, auth_file))
}

pub fn find_auth() -> Option<String> {
    if let Ok(path) = std::env::var("NIRUREMOTE_XAUTHORITY")
        && PathBuf::from(&path).exists()
    {
        return Some(path);
    }
    if let Ok(path) = std::env::var("XAUTHORITY")
        && PathBuf::from(&path).exists()
    {
        return Some(path);
    }

    let mut candidates = vec![home_dir().join(".Xauthority")];
    let runtime = PathBuf::from(format!("/run/user/{}", uid()));
    candidates.push(runtime.join("gdm/Xauthority"));
    candidates.push(runtime.join("lightdm/xauth"));
    candidates.push(runtime.join("sddm/xauth"));
    candidates.push(runtime.join("xauth"));
    candidates
        .into_iter()
        .find(|path| path.exists())
        .map(|path| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_x11_session_properties() {
        let properties =
            parse_properties("Type=x11\nDisplay=:0\nActive=yes\nState=active\nUser=1000\n");
        assert_eq!(properties.get("Type").map(String::as_str), Some("x11"));
        assert_eq!(properties.get("Display").map(String::as_str), Some(":0"));
        assert_eq!(properties.get("Active").map(String::as_str), Some("yes"));
    }

    #[test]
    fn finds_auth_candidate_only_if_present() {
        if let Some(path) = find_auth() {
            assert!(PathBuf::from(path).exists());
        }
    }

    #[test]
    fn parses_x_server_arguments() {
        let args = vec![
            "/usr/lib/xorg/Xorg".to_string(),
            ":0".to_string(),
            "-seat".to_string(),
            "seat0".to_string(),
            "-auth".to_string(),
            "/tmp/does-not-exist".to_string(),
        ];
        let (display, auth) = x_display_from_args(&args).unwrap();
        assert_eq!(display, ":0");
        assert!(auth.is_none() || PathBuf::from(auth.unwrap()).exists());

        let args = vec!["Xorg".to_string(), "-display".to_string(), ":1".to_string()];
        assert_eq!(x_display_from_args(&args).unwrap().0, ":1");
    }
}

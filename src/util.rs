use serde::Serialize;
use serde::de::DeserializeOwned;
use std::fs;
use std::io::Read;
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub struct CappedOutput {
    pub status: Option<ExitStatus>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
}

impl CappedOutput {
    pub fn success(&self) -> bool {
        !self.timed_out && self.status.is_some_and(|status| status.success())
    }

    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

fn spawn_reader<R: Read + Send + 'static>(
    mut reader: R,
    max_bytes: usize,
    tag: u8,
    sender: mpsc::Sender<(u8, Vec<u8>)>,
) {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(count) => {
                    if buffer.len() < max_bytes {
                        let take = count.min(max_bytes - buffer.len());
                        buffer.extend_from_slice(&chunk[..take]);
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        let _ = sender.send((tag, buffer));
    });
}

pub fn run_capped(
    program: &str,
    args: &[&str],
    timeout: Duration,
    max_bytes: usize,
) -> std::io::Result<CappedOutput> {
    run_capped_env(program, args, &[], timeout, max_bytes)
}

pub fn run_capped_env(
    program: &str,
    args: &[&str],
    envs: &[(&str, &str)],
    timeout: Duration,
    max_bytes: usize,
) -> std::io::Result<CappedOutput> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in envs {
        command.env(key, value);
    }
    let mut child = command.spawn()?;

    let (sender, receiver) = mpsc::channel();
    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, max_bytes, 0, sender.clone());
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, max_bytes, 1, sender.clone());
    }
    drop(sender);

    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            break child.wait().ok();
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let collect_deadline = Instant::now() + Duration::from_secs(2);
    for _ in 0..2 {
        let remaining = collect_deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        match receiver.recv_timeout(remaining) {
            Ok((0, bytes)) => stdout = bytes,
            Ok((1, bytes)) => stderr = bytes,
            Ok(_) => {}
            Err(_) => break,
        }
    }

    Ok(CappedOutput {
        status,
        stdout,
        stderr,
        timed_out,
    })
}

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn state_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("NIRUREMOTE_STATE_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(path).join("niruremote");
    }
    home_dir().join(".local/state/niruremote")
}

pub fn cache_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("NIRUREMOTE_CACHE_DIR") {
        return PathBuf::from(path);
    }
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(path).join("niruremote");
    }
    home_dir().join(".cache/niruremote")
}

pub fn ensure_private_dir(path: &Path) -> std::io::Result<()> {
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        ensure_private_dir(parent)?;
    }
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp, bytes)?;
    fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

pub fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    write_private(path, &bytes).map_err(|error| format!("{}: {error}", path.display()))
}

pub fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn hostname() -> String {
    fs::read_to_string("/etc/hostname")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "unknown".into())
}

pub fn username() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".into())
}

pub fn uid() -> u32 {
    unsafe { libc::getuid() }
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

static SSH_AGENT_INIT: std::sync::Once = std::sync::Once::new();

/// Pick a working SSH agent before any SSH child or probe thread starts.
///
/// The NIRUX/GNOME Keyring session often exports SSH_AUTH_SOCK to an empty
/// `openssh_agent` while the unlocked key lives in `gcr/ssh`. Prefer the
/// current socket, but replace it when it has no identities and another
/// standard user socket does.
pub fn ensure_ssh_agent_env() {
    SSH_AGENT_INIT.call_once(|| {
        let current = std::env::var_os("SSH_AUTH_SOCK").map(PathBuf::from);
        if let Some(current) = &current
            && agent_has_identities(current)
        {
            return;
        }

        let uid = uid();
        let mut candidates = Vec::new();
        if let Some(current) = &current {
            candidates.push(current.clone());
        }
        for extra in ["gcr/ssh", "openssh_agent", "keyring/ssh", "ssh-agent"] {
            candidates.push(PathBuf::from(format!("/run/user/{uid}/{extra}")));
        }

        for candidate in candidates {
            if !candidate.exists() {
                continue;
            }
            if agent_has_identities(&candidate) {
                unsafe {
                    std::env::set_var("SSH_AUTH_SOCK", &candidate);
                }
                return;
            }
        }
    });
}

fn agent_has_identities(socket: &Path) -> bool {
    let Some(socket) = socket.to_str() else {
        return false;
    };
    let Ok(output) = run_capped_env(
        "ssh-add",
        &["-l"],
        &[("SSH_AUTH_SOCK", socket)],
        Duration::from_secs(2),
        64 * 1024,
    ) else {
        return false;
    };
    output.success() && !output.stdout_text().trim().is_empty()
}

pub fn find_in_path(name: &str) -> Option<PathBuf> {
    let mut directories = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        directories.extend(std::env::split_paths(&path));
    }
    // Arch and some other distributions put x11vnc and the TigerVNC viewer in
    // /usr/sbin, which is not always in a normal user's PATH. These are fixed
    // system locations, not user-controlled search paths.
    for extra in [
        "/usr/local/sbin",
        "/usr/local/bin",
        "/usr/sbin",
        "/usr/bin",
        "/sbin",
        "/bin",
    ] {
        let directory = PathBuf::from(extra);
        if !directories.contains(&directory) {
            directories.push(directory);
        }
    }
    for directory in directories {
        let candidate = directory.join(name);
        if let Ok(metadata) = fs::metadata(&candidate)
            && metadata.is_file()
            && metadata.permissions().mode() & 0o111 != 0
        {
            return Some(candidate);
        }
    }
    None
}

pub fn free_loopback_port() -> Result<u16, String> {
    let listener =
        TcpListener::bind(("127.0.0.1", 0)).map_err(|error| format!("bind loopback: {error}"))?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|error| format!("loopback address: {error}"))
}

pub fn port_open_ip(ip: &str, port: u16, timeout: Duration) -> bool {
    let Ok(ip) = ip.parse::<IpAddr>() else {
        return false;
    };
    let address = SocketAddr::new(ip, port);
    TcpStream::connect_timeout(&address, timeout).is_ok()
}

pub fn port_open_local(port: u16, timeout: Duration) -> bool {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    TcpStream::connect_timeout(&address, timeout).is_ok()
}

pub fn is_pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if result == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

pub fn process_matches(pid: u32, needle: &str) -> bool {
    if !is_pid_alive(pid) {
        return false;
    }
    let needle = needle.to_lowercase();
    let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
    if comm.to_lowercase().contains(&needle) {
        return true;
    }
    let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
    let cmdline = String::from_utf8_lossy(&cmdline).replace('\0', " ");
    cmdline.to_lowercase().contains(&needle)
}

pub fn terminate_pid(pid: u32, grace: Duration) {
    if !is_pid_alive(pid) {
        return;
    }
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGTERM);
    }
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        if !is_pid_alive(pid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if is_pid_alive(pid) {
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

pub fn is_valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 255
        && !host.starts_with('-')
        && host.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_' | ':' | '%')
        })
}

pub fn trim_dot(value: &str) -> String {
    value.trim_end_matches('.').to_string()
}

pub fn human_age(timestamp: u64) -> String {
    let now = now_secs();
    if timestamp == 0 {
        return "never".into();
    }
    let seconds = now.saturating_sub(timestamp);
    if seconds < 60 {
        format!("{seconds}s ago")
    } else if seconds < 3600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

pub fn human_age_opt(timestamp: Option<u64>) -> String {
    timestamp.map(human_age).unwrap_or_else(|| "never".into())
}

pub fn parse_rfc3339(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.len() < 19 || value.starts_with("0001-") {
        return None;
    }
    let year: i64 = value.get(0..4)?.parse().ok()?;
    let month: i64 = value.get(5..7)?.parse().ok()?;
    let day: i64 = value.get(8..10)?.parse().ok()?;
    let hour: i64 = value.get(11..13)?.parse().ok()?;
    let minute: i64 = value.get(14..16)?.parse().ok()?;
    let second: i64 = value.get(17..19)?.parse().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let days = days_from_civil(year, month, day);
    let timestamp = days * 86_400 + hour * 3_600 + minute * 60 + second;
    u64::try_from(timestamp).ok()
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = (month + 9) % 12;
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

pub fn read_tail(path: &Path, max_bytes: usize) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let start = bytes.len().saturating_sub(max_bytes);
    Some(String::from_utf8_lossy(&bytes[start..]).trim().to_string())
}

pub fn configure_child(command: &mut Command, detach: bool, pdeathsig: bool) {
    unsafe {
        command.pre_exec(move || {
            if detach && libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if pdeathsig && libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_epoch_and_known_dates() {
        assert_eq!(parse_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339("2000-01-01T00:00:00Z"), Some(946_684_800));
        assert_eq!(parse_rfc3339("0001-01-01T00:00:00Z"), None);
        assert_eq!(parse_rfc3339("not a date"), None);
    }

    #[test]
    fn host_validation_rejects_option_injection() {
        assert!(is_valid_host("desktop.tailnet.ts.net"));
        assert!(is_valid_host("100.64.0.1"));
        assert!(is_valid_host("fd7a:115c:a1e0::2737:6654"));
        assert!(!is_valid_host("-oProxyCommand=bad"));
        assert!(!is_valid_host("host name"));
        assert!(!is_valid_host(""));
    }

    #[test]
    fn human_age_is_bounded() {
        let now = now_secs();
        assert_eq!(human_age(0), "never");
        assert!(human_age(now.saturating_sub(10)).ends_with("s ago"));
        assert!(human_age(now.saturating_sub(120)).ends_with("m ago"));
        assert_eq!(trim_dot("host.example.ts.net."), "host.example.ts.net");
    }

    #[test]
    fn loopback_port_is_allocated() {
        let port = free_loopback_port().unwrap();
        assert!(port > 0);
    }
}

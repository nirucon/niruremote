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
        if current.as_deref().is_some_and(agent_has_identities) {
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
        let executable = fs::metadata(&candidate)
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0);
        if executable {
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

/// Stop and reap one of our own children: SIGTERM, a bounded grace period,
/// then SIGKILL. Used only on teardown.
pub fn stop_child(child: &mut std::process::Child, grace: Duration) {
    if matches!(child.try_wait(), Ok(Some(_))) {
        return;
    }
    unsafe {
        libc::kill(child.id() as libc::pid_t, libc::SIGTERM);
    }
    let deadline = Instant::now() + grace;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let _ = child.wait();
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

/// Per-user runtime directory for sockets and one-shot files.
///
/// `$XDG_RUNTIME_DIR` is a per-user tmpfs (0700, cleared at logout). Without
/// it, fall back to the private state directory.
pub fn runtime_dir() -> PathBuf {
    if let Some(path) = std::env::var_os("NIRUREMOTE_RUNTIME_DIR") {
        return PathBuf::from(path);
    }
    if let Some(base) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from) {
        let ours = fs::metadata(&base).is_ok_and(|metadata| {
            use std::os::unix::fs::MetadataExt;
            metadata.is_dir() && metadata.uid() == uid()
        });
        if ours {
            return base.join("niruremote");
        }
    }
    state_dir().join("run")
}

/// Create a file that only the current user can read, failing if it exists.
pub fn create_private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

/// A random alphanumeric secret from the kernel CSPRNG.
pub fn random_secret(length: usize) -> Result<String, String> {
    const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";
    let mut source =
        fs::File::open("/dev/urandom").map_err(|error| format!("/dev/urandom: {error}"))?;
    let mut secret = String::with_capacity(length);
    let mut byte = [0u8; 1];
    // Rejection sampling keeps every character equally likely.
    let limit = 256 - (256 % ALPHABET.len());
    while secret.len() < length {
        source
            .read_exact(&mut byte)
            .map_err(|error| format!("/dev/urandom: {error}"))?;
        if usize::from(byte[0]) < limit {
            secret.push(char::from(ALPHABET[usize::from(byte[0]) % ALPHABET.len()]));
        }
    }
    Ok(secret)
}

/// Replace terminal control characters in text that came from the network
/// (Tailscale names, remote agent JSON) before it is printed.
pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character.is_control() {
                '?'
            } else {
                character
            }
        })
        .collect()
}

struct TcpEntry {
    local: (u32, u16),
    remote: (u32, u16),
    state: u8,
    uid: u32,
}

fn parse_hex_address(text: &str) -> Option<(u32, u16)> {
    let (address, port) = text.split_once(':')?;
    if address.len() != 8 {
        return None;
    }
    let address = u32::from_str_radix(address, 16).ok()?;
    let port = u16::from_str_radix(port, 16).ok()?;
    Some((address, port))
}

fn parse_proc_tcp(text: &str) -> Vec<TcpEntry> {
    let mut entries = Vec::new();
    for line in text.lines().skip(1) {
        let fields: Vec<&str> = line.split_whitespace().collect();
        if fields.len() < 8 {
            continue;
        }
        let (Some(local), Some(remote)) =
            (parse_hex_address(fields[1]), parse_hex_address(fields[2]))
        else {
            continue;
        };
        let (Ok(state), Ok(uid)) = (u8::from_str_radix(fields[3], 16), fields[7].parse::<u32>())
        else {
            continue;
        };
        entries.push(TcpEntry {
            local,
            remote,
            state,
            uid,
        });
    }
    entries
}

/// `/proc/net/tcp` stores IPv4 addresses in host byte order.
const LOOPBACK_HEX: u32 = u32::from_ne_bytes([127, 0, 0, 1]);
const TCP_LISTEN: u8 = 0x0A;

fn proc_tcp() -> Vec<TcpEntry> {
    parse_proc_tcp(&fs::read_to_string("/proc/net/tcp").unwrap_or_default())
}

/// True when something listens on 127.0.0.1:`port`. This reads the kernel
/// table instead of connecting, so it never counts as a VNC client.
pub fn tcp_listening_local(port: u16) -> bool {
    proc_tcp()
        .iter()
        .any(|entry| entry.state == TCP_LISTEN && entry.local == (LOOPBACK_HEX, port))
}

/// The uid that owns the client end of an accepted loopback connection.
pub fn tcp_peer_uid(peer_port: u16, our_port: u16) -> Option<u32> {
    proc_tcp()
        .iter()
        .find(|entry| {
            entry.local == (LOOPBACK_HEX, peer_port) && entry.remote == (LOOPBACK_HEX, our_port)
        })
        .map(|entry| entry.uid)
}

/// The uid of the process on the other end of a Unix socket.
pub fn unix_peer_uid(stream: &std::os::unix::net::UnixStream) -> Option<u32> {
    use std::os::fd::AsRawFd;
    let mut credentials = libc::ucred {
        pid: 0,
        uid: u32::MAX,
        gid: u32::MAX,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    (result == 0).then_some(credentials.uid)
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
    fn proc_net_tcp_is_parsed_in_host_byte_order() {
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 1 1 0000000000000000 100 0 0 10 0
   1: 0100007F:C350 0100007F:1F90 01 00000000:00000000 00:00000000 00000000    65534        0 2 1 0000000000000000 20 4 30 10 -1
   2: 00000000:0016 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 3 1 0000000000000000 100 0 0 10 0
";
        let entries = parse_proc_tcp(table);
        assert_eq!(entries.len(), 3);
        if cfg!(target_endian = "little") {
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.state == TCP_LISTEN && entry.local == (LOOPBACK_HEX, 8080))
            );
            let client = entries
                .iter()
                .find(|entry| entry.local == (LOOPBACK_HEX, 50000))
                .unwrap();
            assert_eq!(client.remote, (LOOPBACK_HEX, 8080));
            assert_eq!(client.uid, 65534);
        }
        assert!(parse_hex_address("zz:1").is_none());
    }

    #[test]
    fn real_loopback_listener_is_seen_without_connecting() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(tcp_listening_local(port));
        let client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (_server, peer) = listener.accept().unwrap();
        assert_eq!(tcp_peer_uid(peer.port(), port), Some(uid()));
        drop(client);
        drop(listener);
        assert!(!tcp_listening_local(port));
    }

    #[test]
    fn unix_peer_is_the_current_user() {
        let (left, _right) = std::os::unix::net::UnixStream::pair().unwrap();
        assert_eq!(unix_peer_uid(&left), Some(uid()));
    }

    #[test]
    fn secrets_are_random_and_alphanumeric() {
        let first = random_secret(8).unwrap();
        let second = random_secret(8).unwrap();
        assert_eq!(first.len(), 8);
        assert!(
            first
                .chars()
                .all(|character| character.is_ascii_alphanumeric())
        );
        assert_ne!(first, second);
    }

    #[test]
    fn sanitize_strips_terminal_escapes() {
        assert_eq!(sanitize("host\u{1b}[31mred\n"), "host?[31mred?");
        assert_eq!(sanitize("desktop · ok"), "desktop · ok");
    }

    #[test]
    fn private_files_are_0600_and_exclusive() {
        let directory =
            std::env::temp_dir().join(format!("niruremote-test-{}", std::process::id()));
        ensure_private_dir(&directory).unwrap();
        let path = directory.join("secret");
        let _ = fs::remove_file(&path);
        create_private_file(&path).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(create_private_file(&path).is_err());
        fs::remove_dir_all(&directory).unwrap();
    }

    #[test]
    fn loopback_port_is_allocated() {
        let port = free_loopback_port().unwrap();
        assert!(port > 0);
    }
}

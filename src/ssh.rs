use crate::json::{LocalState, SCHEMA};
use crate::util::{ensure_private_dir, now_secs, read_tail, run_capped, runtime_dir};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct SshError {
    pub state: String,
    pub detail: String,
}

impl SshError {
    fn new(state: &str, detail: impl Into<String>) -> Self {
        Self {
            state: state.into(),
            detail: detail.into(),
        }
    }
}

fn ssh_bin() -> String {
    std::env::var("NIRUREMOTE_SSH").unwrap_or_else(|_| "ssh".into())
}

/// Options for every SSH connection NIRUREMOTE makes.
///
/// - `BatchMode` never prompts; `StrictHostKeyChecking=yes` fails closed on an
///   unknown or changed host key.
/// - `ClearAllForwardings`, `ForwardAgent=no`, `ForwardX11=no` and
///   `PermitLocalCommand=no` keep a user `~/.ssh/config` from adding
///   listeners, agent or X11 access to these automated connections.
/// - `ControlMaster=no`/`ControlPath=none` keep the session's lifetime equal
///   to this process: no multiplexed master lingers after a disconnect.
fn common_args(timeout: Duration, alive_interval: u32, alive_count: u32) -> Vec<String> {
    let mut args = vec!["-T".to_string()];
    for option in [
        "BatchMode=yes".to_string(),
        format!("ConnectTimeout={}", timeout.as_secs().max(1)),
        format!("ServerAliveInterval={alive_interval}"),
        format!("ServerAliveCountMax={alive_count}"),
        "StrictHostKeyChecking=yes".to_string(),
        "ClearAllForwardings=yes".to_string(),
        "ForwardAgent=no".to_string(),
        "ForwardX11=no".to_string(),
        "PermitLocalCommand=no".to_string(),
        "ControlMaster=no".to_string(),
        "ControlPath=none".to_string(),
        "EscapeChar=none".to_string(),
        "RemoteCommand=none".to_string(),
        "LogLevel=ERROR".to_string(),
    ] {
        args.push("-o".into());
        args.push(option);
    }
    args
}

/// The full argument list for a fixed remote command. The host is validated
/// and placed after `--`; the remote command is constant, never built from
/// host names, user names or tailnet data.
fn command_args(
    host: &str,
    timeout: Duration,
    interval: u32,
    count: u32,
    remote: &[&str],
) -> Vec<String> {
    let mut args = common_args(timeout, interval, count);
    args.push("--".into());
    args.push(host.to_string());
    args.extend(remote.iter().map(|part| part.to_string()));
    args
}

pub const REMOTE_PROBE: &[&str] = &["niruremote", "probe", "--json"];
pub const REMOTE_STREAM: &[&str] = &["niruremote", "stream"];

/// `ssh HOST niruremote stream`: the remote desktop travels over this SSH
/// channel's stdin/stdout. Keepalives end it within ~15 s of a dead path.
pub fn stream_command(host: &str, timeout: Duration) -> Result<Command, SshError> {
    if !crate::util::is_valid_host(host) {
        return Err(SshError::new("invalid-host", "invalid SSH host name"));
    }
    let mut command = Command::new(ssh_bin());
    command.args(command_args(host, timeout, 5, 3, REMOTE_STREAM));
    Ok(command)
}

pub fn probe(host: &str, timeout: Duration) -> Result<LocalState, SshError> {
    if !crate::util::is_valid_host(host) {
        return Err(SshError::new("invalid-host", "invalid SSH host name"));
    }
    let binary = ssh_bin();
    let args = command_args(host, timeout, 2, 2, REMOTE_PROBE);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let output = run_capped(&binary, &refs, timeout + Duration::from_secs(3), 256 * 1024).map_err(
        |error| SshError::new("ssh-unavailable", format!("ssh is not available: {error}")),
    )?;

    if output.timed_out {
        return Err(SshError::new("ssh-timeout", "SSH probe timed out"));
    }
    if !output.success() {
        return Err(classify_ssh_failure(&output));
    }

    let local: LocalState = serde_json::from_slice(&output.stdout).map_err(|error| {
        let preview = output.stdout_text();
        let preview = preview.lines().next().unwrap_or_default();
        SshError::new(
            "protocol-error",
            format!("niruremote agent returned invalid JSON: {error} ({preview})"),
        )
    })?;
    if local.schema != SCHEMA {
        return Err(SshError::new(
            "protocol-error",
            format!("unsupported agent schema {}", local.schema),
        ));
    }
    Ok(local)
}

fn classify_ssh_failure(output: &crate::util::CappedOutput) -> SshError {
    let stderr = output.stderr_text();
    let stdout = output.stdout_text();
    let text = format!("{stdout}\n{stderr}").to_lowercase();
    if text.contains("host key verification failed")
        || text.contains("remote host identification has changed")
        || text.contains("no matching host key")
    {
        return SshError::new(
            "host-key-untrusted",
            "SSH host key is not trusted; verify it in a terminal first",
        );
    }
    if text.contains("permission denied") || text.contains("publickey") {
        return SshError::new(
            "ssh-denied",
            "SSH authentication failed; load the right key or open a terminal",
        );
    }
    if text.contains("command not found")
        || text.contains("no such file or directory")
        || output.status.as_ref().and_then(|status| status.code()) == Some(127)
    {
        return SshError::new(
            "not-installed",
            "NIRUREMOTE is not installed on the remote machine",
        );
    }
    if text.contains("could not resolve")
        || text.contains("connection refused")
        || text.contains("no route to host")
        || text.contains("connection timed out")
        || text.contains("network is unreachable")
    {
        return SshError::new("unreachable", "SSH is not reachable on this machine");
    }
    let detail = stderr.trim();
    if detail.is_empty() {
        SshError::new("ssh-unavailable", "SSH failed without diagnostic output")
    } else {
        SshError::new("ssh-unavailable", detail)
    }
}

/// A private directory holding the stderr of an SSH child, removed on drop.
pub struct TempFiles {
    dir: PathBuf,
    stderr: PathBuf,
}

impl TempFiles {
    pub fn new(prefix: &str) -> Result<Self, String> {
        let dir = runtime_dir().join(format!("{prefix}-{}-{}", std::process::id(), now_secs()));
        ensure_private_dir(&dir).map_err(|error| format!("temp directory: {error}"))?;
        Ok(Self {
            stderr: dir.join("stderr"),
            dir,
        })
    }

    pub fn open_stderr(&self) -> Result<fs::File, String> {
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.stderr)
            .map_err(|error| format!("temp stderr: {error}"))?;
        set_private(&self.stderr)?;
        Ok(file)
    }

    pub fn read_stderr(&self) -> String {
        read_tail(&self.stderr, 8192).unwrap_or_default()
    }
}

impl Drop for TempFiles {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn set_private(path: &std::path::Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("permissions {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_key_checking_is_strict_and_forwardings_are_cleared() {
        let args = command_args(
            "desktop.example.ts.net",
            Duration::from_secs(3),
            5,
            3,
            REMOTE_STREAM,
        );
        let joined = args.join(" ");
        assert!(joined.contains("StrictHostKeyChecking=yes"));
        assert!(!joined.to_lowercase().contains("stricthostkeychecking=no"));
        assert!(!joined.contains("accept-new"));
        assert!(!joined.contains("UserKnownHostsFile"));
        assert!(joined.contains("BatchMode=yes"));
        assert!(joined.contains("ClearAllForwardings=yes"));
        assert!(joined.contains("ForwardAgent=no"));
        assert!(joined.contains("ControlPath=none"));
        assert!(
            !args
                .iter()
                .any(|argument| argument == "-L" || argument == "-R" || argument == "-D")
        );
    }

    #[test]
    fn remote_command_is_fixed_and_after_the_host() {
        let args = command_args("beta", Duration::from_secs(3), 5, 3, REMOTE_STREAM);
        let separator = args.iter().position(|argument| argument == "--").unwrap();
        assert_eq!(args[separator + 1], "beta");
        assert_eq!(&args[separator + 2..], ["niruremote", "stream"]);
        let probe = command_args("beta", Duration::from_secs(3), 2, 2, REMOTE_PROBE);
        assert_eq!(probe.last().map(String::as_str), Some("--json"));
    }

    #[test]
    fn hostile_host_names_are_refused() {
        for host in [
            "-oProxyCommand=x",
            "a;b",
            "a b",
            "$(id)",
            "a`id`",
            "a\nb",
            "",
        ] {
            assert!(
                stream_command(host, Duration::from_secs(1)).is_err(),
                "{host:?}"
            );
        }
    }
}

use crate::json::{EnsureResponse, LocalState, SCHEMA};
use crate::util::{
    configure_child, ensure_private_dir, now_secs, port_open_local, read_tail, run_capped,
    state_dir, terminate_pid,
};
use std::fs;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::{Duration, Instant};

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

fn common_args(timeout: Duration) -> Vec<String> {
    vec![
        "-T".into(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        format!("ConnectTimeout={}", timeout.as_secs().max(1)),
        "-o".into(),
        "ServerAliveInterval=2".into(),
        "-o".into(),
        "ServerAliveCountMax=2".into(),
        "-o".into(),
        "StrictHostKeyChecking=yes".into(),
        "-o".into(),
        "LogLevel=ERROR".into(),
    ]
}

pub fn probe(host: &str, timeout: Duration) -> Result<LocalState, SshError> {
    if !crate::util::is_valid_host(host) {
        return Err(SshError::new("invalid-host", "invalid SSH host name"));
    }
    let binary = ssh_bin();
    let mut args = common_args(timeout);
    args.push(host.to_string());
    args.push("niruremote".into());
    args.push("probe".into());
    args.push("--json".into());
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

struct TempFiles {
    dir: PathBuf,
    stdout: PathBuf,
    stderr: PathBuf,
}

impl TempFiles {
    fn new(prefix: &str) -> Result<Self, String> {
        let dir = state_dir().join(format!("{prefix}-{}-{}", std::process::id(), now_secs()));
        ensure_private_dir(&dir).map_err(|error| format!("temp directory: {error}"))?;
        Ok(Self {
            stdout: dir.join("stdout"),
            stderr: dir.join("stderr"),
            dir,
        })
    }

    fn open_stdout(&self) -> Result<fs::File, String> {
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.stdout)
            .map_err(|error| format!("temp stdout: {error}"))?;
        set_private(&self.stdout)?;
        Ok(file)
    }

    fn open_stderr(&self) -> Result<fs::File, String> {
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&self.stderr)
            .map_err(|error| format!("temp stderr: {error}"))?;
        set_private(&self.stderr)?;
        Ok(file)
    }

    fn read_stdout(&self) -> String {
        fs::read_to_string(&self.stdout).unwrap_or_default()
    }

    fn read_stderr(&self) -> String {
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

pub struct RemoteSession {
    pub response: EnsureResponse,
    child: Child,
    stdin: Option<ChildStdin>,
    _files: TempFiles,
}

impl Drop for RemoteSession {
    fn drop(&mut self) {
        drop(self.stdin.take());
        terminate_pid(self.child.id(), Duration::from_secs(1));
        let _ = self.child.wait();
    }
}

pub fn ensure_remote(host: &str, timeout: Duration) -> Result<RemoteSession, SshError> {
    if !crate::util::is_valid_host(host) {
        return Err(SshError::new("invalid-host", "invalid SSH host name"));
    }
    let binary = ssh_bin();
    let mut args = common_args(timeout);
    args.push(host.to_string());
    args.push("niruremote".into());
    args.push("ensure".into());
    args.push("--json".into());
    args.push("--hold".into());
    let mut command = Command::new(binary);
    command.args(&args);
    spawn_ensure(command, timeout + Duration::from_secs(3))
}

pub fn ensure_local(timeout: Duration) -> Result<RemoteSession, SshError> {
    let binary = std::env::current_exe()
        .map_err(|error| SshError::new("local-error", format!("current executable: {error}")))?;
    let mut command = Command::new(binary);
    command.args(["ensure", "--json", "--hold"]);
    spawn_ensure(command, timeout + Duration::from_secs(3))
}

fn spawn_ensure(mut command: Command, timeout: Duration) -> Result<RemoteSession, SshError> {
    let files = TempFiles::new("ensure").map_err(|error| SshError::new("local-error", error))?;
    let stdout = files
        .open_stdout()
        .map_err(|error| SshError::new("local-error", error))?;
    let stderr = files
        .open_stderr()
        .map_err(|error| SshError::new("local-error", error))?;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::from(stdout))
        .stderr(Stdio::from(stderr));
    configure_child(&mut command, false, true);

    let mut child = command.spawn().map_err(|error| {
        SshError::new("ssh-unavailable", format!("could not start SSH: {error}"))
    })?;
    let stdin = child.stdin.take();
    let deadline = Instant::now() + timeout;

    loop {
        let stdout = files.read_stdout();
        if let Some(line) = stdout.lines().find(|line| !line.trim().is_empty()) {
            match serde_json::from_str::<EnsureResponse>(line.trim()) {
                Ok(response) => {
                    if response.local.state != "ready" {
                        let detail = response
                            .local
                            .last_error
                            .clone()
                            .unwrap_or_else(|| "remote agent is not ready".into());
                        let state = response.local.state.clone();
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(SshError::new(&state, detail));
                    }
                    if response.local.schema != SCHEMA {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(SshError::new(
                            "protocol-error",
                            format!("unsupported agent schema {}", response.local.schema),
                        ));
                    }
                    return Ok(RemoteSession {
                        response,
                        child,
                        stdin,
                        _files: files,
                    });
                }
                Err(_) => {
                    if stdout.contains('}') {
                        let detail = format!(
                            "invalid ensure response: {}",
                            stdout.lines().next().unwrap_or_default()
                        );
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(SshError::new("protocol-error", detail));
                    }
                }
            }
        }

        if let Ok(Some(status)) = child.try_wait() {
            let stderr = files.read_stderr();
            let detail = if stderr.is_empty() {
                format!("remote ensure command exited with {status}")
            } else {
                stderr
            };
            return Err(SshError::new("agent-error", detail));
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let stderr = files.read_stderr();
            return Err(SshError::new(
                "ssh-timeout",
                if stderr.is_empty() {
                    "timed out waiting for the remote agent".into()
                } else {
                    stderr
                },
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

pub struct Tunnel {
    child: Child,
    _files: TempFiles,
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        terminate_pid(self.child.id(), Duration::from_secs(1));
        let _ = self.child.wait();
    }
}

pub fn tunnel(host: &str, local_port: u16, remote_port: u16) -> Result<Tunnel, String> {
    if !crate::util::is_valid_host(host) {
        return Err("invalid SSH host name".into());
    }
    let files = TempFiles::new("tunnel")?;
    let stderr = files.open_stderr()?;
    let mut command = Command::new(ssh_bin());
    command
        .arg("-T")
        .arg("-N")
        .arg("-L")
        .arg(format!("127.0.0.1:{local_port}:127.0.0.1:{remote_port}"))
        .arg("-o")
        .arg("ExitOnForwardFailure=yes")
        .arg("-o")
        .arg("BatchMode=yes")
        .arg("-o")
        .arg("ConnectTimeout=5")
        .arg("-o")
        .arg("ServerAliveInterval=5")
        .arg("-o")
        .arg("ServerAliveCountMax=2")
        .arg("-o")
        .arg("StrictHostKeyChecking=yes")
        .arg("-o")
        .arg("LogLevel=ERROR")
        .arg(host)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(stderr));
    configure_child(&mut command, false, true);

    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start SSH tunnel: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            let detail = files.read_stderr();
            return Err(if detail.is_empty() {
                format!("SSH tunnel exited with {status}")
            } else {
                format!("SSH tunnel exited with {status}: {detail}")
            });
        }
        if port_open_local(local_port, Duration::from_millis(200)) {
            return Ok(Tunnel {
                child,
                _files: files,
            });
        }
        if Instant::now() >= deadline {
            terminate_pid(child.id(), Duration::from_secs(1));
            let _ = child.wait();
            let detail = files.read_stderr();
            return Err(if detail.is_empty() {
                "SSH tunnel did not become ready".into()
            } else {
                format!("SSH tunnel did not become ready: {detail}")
            });
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

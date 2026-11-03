use crate::json::{PeerCache, RuntimeState, SCHEMA};
use crate::util::{
    cache_dir, ensure_private_dir, is_pid_alive, now_secs, read_json, state_dir, write_json,
};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub fn runtime_path() -> PathBuf {
    state_dir().join("state.json")
}

pub fn log_path() -> PathBuf {
    state_dir().join("x11vnc.log")
}

pub fn cache_path() -> PathBuf {
    cache_dir().join("peers.json")
}

fn lock_path() -> PathBuf {
    state_dir().join("state.lock")
}

pub struct StateLock {
    path: PathBuf,
    held: bool,
}

impl StateLock {
    pub fn acquire() -> Result<Self, String> {
        ensure_private_dir(&state_dir()).map_err(|error| format!("state directory: {error}"))?;
        let path = lock_path();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut file) => {
                    let _ = writeln!(file, "{}", std::process::id());
                    return Ok(Self { path, held: true });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if let Ok(metadata) = fs::metadata(&path)
                        && let Ok(modified) = metadata.modified()
                        && modified.elapsed().unwrap_or_default() > Duration::from_secs(30)
                    {
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    if Instant::now() >= deadline {
                        return Err("state is locked by another niruremote process".into());
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(format!("state lock: {error}")),
            }
        }
    }
}

impl Drop for StateLock {
    fn drop(&mut self) {
        if self.held {
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn read_runtime() -> Option<RuntimeState> {
    let runtime: RuntimeState = read_json(&runtime_path())?;
    if runtime.schema != SCHEMA {
        return None;
    }
    Some(runtime)
}

pub fn valid_runtime() -> Option<RuntimeState> {
    let runtime = read_runtime()?;
    if is_pid_alive(runtime.pid) {
        Some(runtime)
    } else {
        let _ = fs::remove_file(runtime_path());
        None
    }
}

pub fn write_runtime(runtime: &RuntimeState) -> Result<(), String> {
    write_json(&runtime_path(), runtime)
}

pub fn remove_runtime() {
    let _ = fs::remove_file(runtime_path());
}

pub fn new_runtime(
    pid: u32,
    port: u16,
    display: String,
    auth_file: Option<String>,
    backend: String,
    owner: &str,
) -> RuntimeState {
    RuntimeState {
        schema: SCHEMA,
        pid,
        port,
        display,
        auth_file,
        backend,
        started_at: now_secs(),
        owner: owner.into(),
    }
}

pub fn read_cache() -> PeerCache {
    let mut cache: PeerCache = read_json(&cache_path()).unwrap_or_default();
    if cache.schema != SCHEMA {
        cache = PeerCache {
            schema: SCHEMA,
            peers: Default::default(),
        };
    }
    cache
}

pub fn write_cache(cache: &PeerCache) -> Result<(), String> {
    write_json(&cache_path(), cache)
}

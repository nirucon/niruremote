use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SCHEMA: u32 = 1;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SessionInfo {
    #[serde(rename = "type")]
    pub session_type: String,
    pub display: Option<String>,
    pub active: bool,
    pub source: String,
    pub auth_file: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct BackendInfo {
    pub name: String,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub install_hint: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalState {
    pub schema: u32,
    pub agent_version: String,
    pub hostname: String,
    pub user: String,
    pub os: String,
    pub session: SessionInfo,
    pub backend: BackendInfo,
    pub state: String,
    pub running: bool,
    pub enabled: bool,
    pub port: Option<u16>,
    pub pid: Option<u32>,
    pub since: Option<u64>,
    pub mode: String,
    pub owner: Option<String>,
    pub last_error: Option<String>,
    pub supported_modes: Vec<String>,
    /// Screen locker running in the shared session (`nirulock`, ...).
    #[serde(default)]
    pub locker: Option<String>,
    /// Live `connect` sessions served by this agent.
    #[serde(default)]
    pub sessions: u32,
}

/// Remote-desktop mode served over the SSH channel itself (0.2.0+).
pub const MODE_STREAM: &str = "ssh-stdio";

impl LocalState {
    pub fn new() -> Self {
        Self {
            schema: SCHEMA,
            agent_version: env!("CARGO_PKG_VERSION").into(),
            hostname: String::new(),
            user: String::new(),
            os: std::env::consts::OS.into(),
            session: SessionInfo::default(),
            backend: BackendInfo::default(),
            state: "error".into(),
            running: false,
            enabled: false,
            port: None,
            pid: None,
            since: None,
            mode: "loopback".into(),
            owner: None,
            last_error: None,
            supported_modes: Vec::new(),
            locker: None,
            sessions: 0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeState {
    pub schema: u32,
    pub pid: u32,
    pub port: u16,
    pub display: String,
    pub auth_file: Option<String>,
    pub backend: String,
    pub started_at: u64,
    pub owner: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EnsureResponse {
    #[serde(flatten)]
    pub local: LocalState,
    pub owned: bool,
    pub log: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peer {
    pub id: String,
    pub host: String,
    pub dns_name: Option<String>,
    pub ip: Option<String>,
    pub os: String,
    pub online: bool,
    pub last_seen: Option<String>,
    pub last_seen_epoch: Option<u64>,
    pub user: Option<String>,
    pub user_id: Option<i64>,
    pub tags: Vec<String>,
    pub is_self: bool,
    pub exit_node_option: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Snapshot {
    pub backend_state: String,
    pub peers: Vec<Peer>,
    pub self_peer: Option<Peer>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SshView {
    pub state: String,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteView {
    pub state: String,
    pub detail: String,
    pub mode: Option<String>,
    pub version: Option<String>,
    pub checked_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeerView {
    #[serde(flatten)]
    pub peer: Peer,
    pub ssh: SshView,
    pub remote: RemoteView,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PeersResponse {
    pub schema: u32,
    pub refreshed_at: u64,
    pub peers: Vec<PeerView>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CachedPeer {
    pub state: String,
    pub detail: String,
    pub mode: Option<String>,
    pub version: Option<String>,
    pub ssh_state: String,
    pub ssh_detail: String,
    pub checked_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PeerCache {
    pub schema: u32,
    pub peers: BTreeMap<String, CachedPeer>,
}

use crate::json::{Peer, Snapshot};
use crate::util::{parse_rfc3339, run_capped, trim_dot};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

fn tailscale_bin() -> String {
    std::env::var("NIRUREMOTE_TAILSCALE").unwrap_or_else(|_| "tailscale".into())
}

pub fn status() -> Result<Snapshot, String> {
    let binary = tailscale_bin();
    let output = run_capped(
        &binary,
        &["status", "--json"],
        Duration::from_secs(3),
        2 * 1024 * 1024,
    )
    .map_err(|error| format!("tailscale is not available: {error}"))?;

    if output.timed_out {
        return Err("tailscale status timed out".into());
    }
    if !output.success() {
        let detail = output.stderr_text();
        let detail = detail.trim();
        if detail.is_empty() {
            return Err("tailscale is not running or not logged in".into());
        }
        return Err(format!(
            "tailscale is not running or not logged in: {detail}"
        ));
    }

    parse(&output.stdout)
}

fn string(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::to_string)
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn integer(value: &Value, key: &str) -> Option<i64> {
    let value = value.get(key)?;
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
}

fn parse_users(value: &Value) -> HashMap<i64, String> {
    let mut users = HashMap::new();
    if let Some(object) = value.get("User").and_then(Value::as_object) {
        for (id, user) in object {
            let Ok(id) = id.parse::<i64>() else {
                continue;
            };
            if let Some(login) = user
                .get("LoginName")
                .and_then(Value::as_str)
                .or_else(|| user.get("DisplayName").and_then(Value::as_str))
            {
                users.insert(id, login.to_string());
            }
        }
    }
    users
}

fn parse_peer(key: &str, value: &Value, users: &HashMap<i64, String>, is_self: bool) -> Peer {
    let dns_name = string(value, "DNSName")
        .map(|name| trim_dot(&name))
        .filter(|name| !name.is_empty());
    let host = dns_name
        .clone()
        .or_else(|| string(value, "HostName"))
        .unwrap_or_else(|| key.to_string());
    let ip = value
        .get("TailscaleIPs")
        .and_then(Value::as_array)
        .and_then(|addresses| {
            let mut fallback = None;
            for address in addresses {
                let Some(address) = address.as_str() else {
                    continue;
                };
                if fallback.is_none() {
                    fallback = Some(address.to_string());
                }
                if address
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_ipv4())
                {
                    return Some(address.to_string());
                }
            }
            fallback
        });
    let last_seen = string(value, "LastSeen").filter(|value| !value.starts_with("0001-"));
    let last_seen_epoch = last_seen.as_deref().and_then(parse_rfc3339);
    let user_id = integer(value, "UserID");
    let user = user_id.and_then(|id| users.get(&id).cloned());
    let tags = value
        .get("Tags")
        .and_then(Value::as_array)
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();

    Peer {
        id: key.to_string(),
        host,
        dns_name,
        ip,
        os: string(value, "OS").unwrap_or_else(|| "unknown".into()),
        online: bool_field(value, "Online"),
        last_seen,
        last_seen_epoch,
        user,
        user_id,
        tags,
        is_self,
        exit_node_option: bool_field(value, "ExitNodeOption"),
    }
}

pub fn parse(bytes: &[u8]) -> Result<Snapshot, String> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|error| format!("tailscale JSON: {error}"))?;
    let users = parse_users(&value);
    let self_peer = value
        .get("Self")
        .map(|self_value| parse_peer("self", self_value, &users, true));
    let self_id = self_peer.as_ref().map(|peer| peer.id.clone());

    let mut peers = Vec::new();
    if let Some(object) = value.get("Peer").and_then(Value::as_object) {
        for (key, peer) in object {
            peers.push(parse_peer(
                key,
                peer,
                &users,
                self_id.as_deref() == Some(key.as_str()),
            ));
        }
    }
    peers.sort_by(|left, right| {
        right
            .online
            .cmp(&left.online)
            .then_with(|| left.host.cmp(&right.host))
    });

    Ok(Snapshot {
        backend_state: value
            .get("BackendState")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        peers,
        self_peer,
    })
}

pub fn all_peers(snapshot: &Snapshot, include_self: bool) -> Vec<Peer> {
    let mut peers = snapshot.peers.clone();
    if include_self {
        peers.extend(snapshot.self_peer.clone());
    }
    peers.sort_by(|left, right| {
        right
            .online
            .cmp(&left.online)
            .then_with(|| left.host.cmp(&right.host))
    });
    peers
}

#[cfg(test)]
mod tests {
    use super::*;

    const BODY: &[u8] = br#"{
      "BackendState":"Running",
      "Self": {
        "HostName":"me",
        "DNSName":"me.example.ts.net.",
        "OS":"linux",
        "UserID":7,
        "Online":true,
        "TailscaleIPs":["100.64.0.1","fd7a::1"]
      },
      "User": {
        "7": {"LoginName":"me@example.com"},
        "8": {"LoginName":"other@example.com"}
      },
      "Peer": {
        "node-b": {
          "HostName":"beta",
          "DNSName":"beta.example.ts.net.",
          "OS":"linux",
          "UserID":7,
          "Online":true,
          "TailscaleIPs":["100.64.0.2","fd7a::2"],
          "Tags":["tag:desktop"]
        },
        "node-a": {
          "HostName":"alpha",
          "DNSName":"alpha.example.ts.net.",
          "OS":"windows",
          "UserID":8,
          "Online":false,
          "LastSeen":"2026-11-02T18:00:00Z"
        },
        "node-c": {
          "HostName":"headless",
          "OS":"linux",
          "UserID":7,
          "Online":true,
          "TailscaleIPs":["100.64.0.3"],
          "ExitNodeOption":true
        }
      }
    }"#;

    #[test]
    fn peers_are_parsed_with_os_offline_and_identity() {
        let snapshot = parse(BODY).unwrap();
        assert_eq!(snapshot.backend_state, "Running");
        assert_eq!(snapshot.peers.len(), 3);

        let alpha = snapshot
            .peers
            .iter()
            .find(|peer| peer.host == "alpha.example.ts.net")
            .unwrap();
        assert_eq!(alpha.os, "windows");
        assert!(!alpha.online);
        assert_eq!(alpha.user.as_deref(), Some("other@example.com"));
        assert!(alpha.last_seen_epoch.is_some());

        let beta = snapshot
            .peers
            .iter()
            .find(|peer| peer.host == "beta.example.ts.net")
            .unwrap();
        assert_eq!(beta.ip.as_deref(), Some("100.64.0.2"));
        assert_eq!(beta.user.as_deref(), Some("me@example.com"));
        assert_eq!(beta.tags, vec!["tag:desktop"]);
        assert!(beta.online);

        let headless = snapshot
            .peers
            .iter()
            .find(|peer| peer.host == "headless")
            .unwrap();
        assert!(headless.exit_node_option);
    }

    #[test]
    fn online_peers_sort_first() {
        let snapshot = parse(BODY).unwrap();
        assert_eq!(snapshot.peers[0].host, "beta.example.ts.net");
        assert_eq!(snapshot.peers[1].host, "headless");
        assert_eq!(
            snapshot.peers.last().map(|peer| peer.host.as_str()),
            Some("alpha.example.ts.net")
        );
    }
}

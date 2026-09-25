//! The LAN-announce capability: emit a Minecraft "open to LAN" broadcast on
//! the local network so the user's own Minecraft client discovers a world that
//! is really being tunnelled from somewhere else.
//!
//! Minecraft discovers LAN games by listening for UDP multicast on
//! `224.0.2.60:4445`; the packet body is `[MOTD]<name>[/MOTD][AD]<port>[/AD]`.
//! A webview cannot open a multicast socket, so this has to live natively. It
//! is gated on the `lan` permission — a plugin that holds it can put a joinable
//! entry into every Minecraft client on the local network.

use serde::Serialize;
use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::Mutex;

/// The multicast group and port Minecraft's LAN discovery listens on.
const MULTICAST_ADDR: Ipv4Addr = Ipv4Addr::new(224, 0, 2, 60);
const MULTICAST_PORT: u16 = 4445;
const ANNOUNCE_INTERVAL: Duration = Duration::from_millis(1500);

struct Announcer {
    handle: u64,
    task: tokio::task::JoinHandle<()>,
}

static REGISTRY: LazyLock<Mutex<HashMap<String, Vec<Announcer>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize)]
pub struct LanAnnounce {
    pub handle: u64,
}

/// Begin announcing a LAN world on `port` under `motd`, repeating until
/// [`stop`] or [`cleanup`]. Returns a handle to stop this announcer.
pub async fn announce(
    plugin_id: &str,
    motd: &str,
    port: u16,
) -> crate::Result<LanAnnounce> {
    super::store::ensure_granted(plugin_id, "lan").await?;

    if port == 0 {
        return Err(crate::ErrorKind::InputError(
            "LAN announce port must not be zero".to_string(),
        )
        .into());
    }

    // Keep the delimiters intact: a bracket in the name would break the packet.
    let motd = motd.replace(['[', ']'], "");
    let payload = format!("[MOTD]{motd}[/MOTD][AD]{port}[/AD]");

    let task = tokio::spawn(async move {
        let socket = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
            Ok(socket) => socket,
            Err(error) => {
                tracing::warn!("LAN announcer could not bind a socket: {error}");
                return;
            }
        };
        let _ = socket.set_multicast_loop_v4(true);
        let _ = socket.set_multicast_ttl_v4(1);

        loop {
            if let Err(error) = socket
                .send_to(payload.as_bytes(), (MULTICAST_ADDR, MULTICAST_PORT))
                .await
            {
                tracing::debug!("LAN announce send failed: {error}");
            }
            tokio::time::sleep(ANNOUNCE_INTERVAL).await;
        }
    });

    let handle = NEXT_HANDLE.fetch_add(1, Ordering::Relaxed);
    REGISTRY
        .lock()
        .await
        .entry(plugin_id.to_string())
        .or_default()
        .push(Announcer { handle, task });
    Ok(LanAnnounce { handle })
}

/// Stop one announcer of a plugin.
pub async fn stop(plugin_id: &str, handle: u64) -> crate::Result<()> {
    let mut registry = REGISTRY.lock().await;
    if let Some(list) = registry.get_mut(plugin_id)
        && let Some(index) =
            list.iter().position(|announcer| announcer.handle == handle)
    {
        list.remove(index).task.abort();
    }
    Ok(())
}

/// Stop every announcer a plugin started — called when it unloads or is disabled.
pub async fn cleanup(plugin_id: &str) {
    let mut registry = REGISTRY.lock().await;
    if let Some(list) = registry.remove(plugin_id) {
        for announcer in list {
            announcer.task.abort();
        }
    }
}

/// Stop every announcer of every plugin — called on app shutdown.
pub async fn shutdown_all() {
    let mut registry = REGISTRY.lock().await;
    for (_, list) in registry.drain() {
        for announcer in list {
            announcer.task.abort();
        }
    }
}


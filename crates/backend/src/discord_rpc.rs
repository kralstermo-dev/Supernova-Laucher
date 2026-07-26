//! A minimal implementation of Discord's local IPC protocol for Rich
//! Presence, since no `discord-sdk`/`discord-rich-presence` crate is
//! available to depend on here. The protocol itself is simple, stable, and
//! widely reverse-engineered/documented: a length-prefixed JSON frame
//! protocol over a local named pipe (Windows) or Unix domain socket
//! (macOS/Linux).
//!
//! Frame format: `<opcode: u32 LE><length: u32 LE><json payload>`
//! Opcodes: 0 = HANDSHAKE, 1 = FRAME, 2 = CLOSE, 3 = PING, 4 = PONG.

use std::time::Duration;

use serde::Serialize;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc::UnboundedReceiver;

/// The Discord Application's client ID for Rich Presence. Register an
/// application at https://discord.com/developers/applications and set
/// DISCORD_RPC_CLIENT_ID at build time, or fill in the fallback below.
/// Rich Presence is a no-op (silently disabled) if this is left unset.
fn client_id() -> Option<&'static str> {
    let id = option_env!("DISCORD_RPC_CLIENT_ID").unwrap_or("");
    if id.is_empty() { None } else { Some(id) }
}

/// The state we want Discord to be showing, as computed by the backend.
/// Sent over an unbounded channel; the background task coalesces to
/// whatever the latest value is and rate-limits actual sends to Discord.
#[derive(Debug, Clone, PartialEq)]
pub enum DesiredPresence {
    Idle,
    Playing { instance_name: String, started_at_unix: i64 },
    /// Explicitly clears any Rich Presence Discord is currently showing —
    /// used when the feature gets disabled.
    Cleared,
}

/// A cheap, cloneable handle for feeding presence updates into the
/// background connection task. Sending never blocks and is safe to call
/// from the backend's per-second tick.
#[derive(Clone)]
pub struct DiscordRpcHandle {
    tx: tokio::sync::mpsc::UnboundedSender<DesiredPresence>,
}

impl DiscordRpcHandle {
    pub fn set_presence(&self, presence: DesiredPresence) {
        let _ = self.tx.send(presence);
    }
}

/// Creates a handle plus the not-yet-spawned background task. Split like this
/// (rather than spawning internally) because `BackendState` is constructed
/// before a Tokio runtime context is entered — the returned future must be
/// handed to `runtime.spawn(...)` once inside one.
pub fn create() -> (DiscordRpcHandle, impl std::future::Future<Output = ()>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (DiscordRpcHandle { tx }, run(rx))
}

const RECONNECT_DELAY: Duration = Duration::from_secs(10);
const MIN_UPDATE_INTERVAL: Duration = Duration::from_secs(15);

async fn run(mut rx: UnboundedReceiver<DesiredPresence>) {
    let Some(client_id) = client_id() else { return };

    let mut current: DesiredPresence = DesiredPresence::Idle;

    loop {
        let mut socket = match connect().await {
            Ok(socket) => socket,
            Err(err) => {
                log::debug!("Discord RPC: no Discord client found ({err}), retrying later");
                tokio::time::sleep(RECONNECT_DELAY).await;
                continue;
            },
        };

        if let Err(err) = handshake(&mut socket, client_id).await {
            log::debug!("Discord RPC: handshake failed: {err}");
            tokio::time::sleep(RECONNECT_DELAY).await;
            continue;
        }

        log::info!("Discord RPC: connected");
        let mut last_sent: Option<DesiredPresence> = None;

        // Connected: keep pushing presence updates (rate-limited) until the
        // connection drops, then fall back to the outer reconnect loop.
        let mut throttle = tokio::time::interval(MIN_UPDATE_INTERVAL);
        throttle.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        let mut read_buf = [0u8; 1];
        loop {
            tokio::select! {
                update = rx.recv() => {
                    match update {
                        Some(presence) => current = presence,
                        None => return, // Sender dropped: backend is shutting down.
                    }
                },
                _ = throttle.tick() => {
                    if last_sent.as_ref() != Some(&current) {
                        if let Err(err) = set_activity(&mut socket, &current).await {
                            log::debug!("Discord RPC: failed to update presence, reconnecting: {err}");
                            break;
                        }
                        last_sent = Some(current.clone());
                    }
                },
                // Detect the pipe closing (Discord quit/restarted) so we
                // reconnect promptly instead of waiting for the next write.
                result = socket.read(&mut read_buf) => {
                    match result {
                        Ok(0) | Err(_) => {
                            log::debug!("Discord RPC: connection closed");
                            break;
                        },
                        Ok(_) => {}, // Unexpected inbound data; ignore.
                    }
                },
            }
        }
    }
}

async fn handshake(socket: &mut Socket, client_id: &str) -> anyhow::Result<()> {
    write_frame(socket, 0, &json!({ "v": 1, "client_id": client_id })).await?;
    let (_opcode, _payload) = read_frame(socket).await?;
    Ok(())
}

async fn set_activity(socket: &mut Socket, presence: &DesiredPresence) -> anyhow::Result<()> {
    let activity = match presence {
        DesiredPresence::Cleared => {
            json!({
                "cmd": "SET_ACTIVITY",
                "args": { "pid": std::process::id(), "activity": null },
                "nonce": nonce(),
            })
        },
        DesiredPresence::Idle => {
            json!({
                "cmd": "SET_ACTIVITY",
                "args": {
                    "pid": std::process::id(),
                    "activity": {
                        "details": "In the launcher",
                        "assets": { "large_image": "supernova_logo", "large_text": "Supernova Launcher" },
                    },
                },
                "nonce": nonce(),
            })
        },
        DesiredPresence::Playing { instance_name, started_at_unix } => {
            json!({
                "cmd": "SET_ACTIVITY",
                "args": {
                    "pid": std::process::id(),
                    "activity": {
                        "details": format!("Playing {instance_name}"),
                        "state": "In game",
                        "timestamps": { "start": started_at_unix },
                        "assets": { "large_image": "supernova_logo", "large_text": "Supernova Launcher" },
                    },
                },
                "nonce": nonce(),
            })
        },
    };

    write_frame(socket, 1, &activity).await?;
    Ok(())
}

fn nonce() -> String {
    use rand::RngCore;
    format!("{:016x}", rand::thread_rng().next_u64())
}

async fn write_frame(socket: &mut Socket, opcode: u32, payload: &impl Serialize) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec(payload)?;
    socket.write_all(&opcode.to_le_bytes()).await?;
    socket.write_all(&(bytes.len() as u32).to_le_bytes()).await?;
    socket.write_all(&bytes).await?;
    socket.flush().await?;
    Ok(())
}

async fn read_frame(socket: &mut Socket) -> anyhow::Result<(u32, Vec<u8>)> {
    let mut opcode_buf = [0u8; 4];
    socket.read_exact(&mut opcode_buf).await?;
    let opcode = u32::from_le_bytes(opcode_buf);

    let mut len_buf = [0u8; 4];
    socket.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;

    let mut payload = vec![0u8; len];
    socket.read_exact(&mut payload).await?;

    Ok((opcode, payload))
}

#[cfg(windows)]
type Socket = tokio::net::windows::named_pipe::NamedPipeClient;

#[cfg(windows)]
async fn connect() -> anyhow::Result<Socket> {
    use tokio::net::windows::named_pipe::ClientOptions;

    for i in 0..10 {
        let path = format!(r"\\.\pipe\discord-ipc-{i}");
        match ClientOptions::new().open(&path) {
            Ok(client) => return Ok(client),
            Err(err) if err.raw_os_error() == Some(231) => {
                // ERROR_PIPE_BUSY: a client is already connected on this slot, try the next one.
                continue;
            },
            Err(_) => continue,
        }
    }

    anyhow::bail!("no Discord IPC pipe found (is Discord running?)")
}

#[cfg(unix)]
type Socket = tokio::net::UnixStream;

#[cfg(unix)]
async fn connect() -> anyhow::Result<Socket> {
    let base_dirs = [
        std::env::var("XDG_RUNTIME_DIR").ok(),
        std::env::var("TMPDIR").ok(),
        Some("/tmp".to_string()),
    ];

    for base in base_dirs.into_iter().flatten() {
        for i in 0..10 {
            let path = format!("{base}/discord-ipc-{i}");
            if let Ok(stream) = tokio::net::UnixStream::connect(&path).await {
                return Ok(stream);
            }
        }
    }

    anyhow::bail!("no Discord IPC socket found (is Discord running?)")
}

#[cfg(not(any(windows, unix)))]
compile_error!("Discord RPC is only implemented for Windows and Unix platforms");

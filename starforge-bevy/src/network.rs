//! STARFORGE native multiplayer.
//!
//! This is intentionally a Bevy-only protocol. A lightweight authoritative UDP host
//! relays validated player snapshots, chat and voxel edits. It does not implement or
//! depend on the archived browser/Node.js protocol.

use crate::factory::Machine;
use crate::player::Player;
use crate::space::{FlightMode, ShipState, SpaceGame};
use crate::ui::{Panel, UiState};
use crate::world::World;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::Mutex;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_ADDR: &str = "127.0.0.1:17889";
const PROTOCOL_VERSION: u16 = 3;
const MAX_PACKET: usize = 60_000;
const MAX_PLAYERS: usize = 32;
const MAX_BLOCK_LOG: usize = 100_000;
const MAX_PENDING_BLOCKS: usize = 20_000;
const MAX_BLOCKS_PER_SECOND: usize = 1_000;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NetPlayer {
    pub id: u64,
    pub name: String,
    pub pos: [f32; 3],
    pub yaw: f32,
    /// 0 ground, 1 atmosphere, 2 space/warp, 3 station.
    pub mode: u8,
    pub galaxy: u32,
    pub planet: usize,
    pub seq: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlockUpdate {
    /// Monotonic server-assigned revision. Client submissions use zero.
    #[serde(default)]
    pub seq: u64,
    pub galaxy: u32,
    pub planet: usize,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub id: u8,
    pub dir: u8,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ClientPacket {
    Hello {
        version: u16,
        name: String,
        world_id: u64,
    },
    State {
        player: NetPlayer,
    },
    Chat {
        text: String,
    },
    Block {
        update: BlockUpdate,
    },
    Ping,
    Disconnect,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ServerPacket {
    Welcome { id: u64, host_name: String },
    Reject { reason: String },
    Snapshot { seq: u64, players: Vec<NetPlayer> },
    Chat { name: String, text: String },
    Notice { text: String },
    Block { update: BlockUpdate },
    WorldDelta { blocks: Vec<BlockUpdate> },
    Pong,
}

#[derive(Clone, Debug)]
enum ClientCommand {
    Packet(ClientPacket),
    Stop,
}

#[derive(Clone, Debug)]
enum ClientEvent {
    Packet(ServerPacket),
    Error(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectionRole {
    Offline,
    Host,
    Client,
}

impl ConnectionRole {
    fn label(self) -> &'static str {
        match self {
            Self::Offline => "离线",
            Self::Host => "主机",
            Self::Client => "成员",
        }
    }
}

#[derive(Component)]
pub struct RemoteAvatar {
    id: u64,
    target: Vec3,
    yaw: f32,
    mode: u8,
    galaxy: u32,
    planet: usize,
}

#[derive(Message, Clone, Copy, Debug)]
pub struct BlockChanged {
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub id: u8,
    pub dir: u8,
}

#[derive(Resource)]
pub struct NetworkState {
    pub address: String,
    pub name: String,
    pub status: String,
    pub role: ConnectionRole,
    pub connected: bool,
    pub my_id: u64,
    pub players: Vec<NetPlayer>,
    pub chat_input: String,
    pub chat: Vec<String>,
    command: Option<Sender<ClientCommand>>,
    events: Option<Mutex<Receiver<ClientEvent>>>,
    server_stop: Option<Sender<()>>,
    remote_entities: HashMap<u64, Entity>,
    pending_blocks: HashMap<(u32, usize), Vec<BlockUpdate>>,
    block_versions: HashMap<(u32, usize, i32, i32, i32), u64>,
    snapshot_seq: u64,
    send_acc: f32,
    seq: u32,
}

impl Default for NetworkState {
    fn default() -> Self {
        Self {
            address: DEFAULT_ADDR.to_string(),
            name: "探险家".to_string(),
            status: "未连接".to_string(),
            role: ConnectionRole::Offline,
            connected: false,
            my_id: 0,
            players: Vec::new(),
            chat_input: String::new(),
            chat: Vec::new(),
            command: None,
            events: None,
            server_stop: None,
            remote_entities: HashMap::new(),
            pending_blocks: HashMap::new(),
            block_versions: HashMap::new(),
            snapshot_seq: 0,
            send_acc: 0.0,
            seq: 0,
        }
    }
}

impl NetworkState {
    fn send(&self, packet: ClientPacket) {
        if let Some(tx) = &self.command {
            let _ = tx.send(ClientCommand::Packet(packet));
        }
    }

    fn stop_transport(&mut self) {
        if let Some(tx) = self.command.take() {
            let _ = tx.send(ClientCommand::Packet(ClientPacket::Disconnect));
            let _ = tx.send(ClientCommand::Stop);
        }
        if let Some(stop) = self.server_stop.take() {
            let _ = stop.send(());
        }
        self.events = None;
        self.connected = false;
        self.my_id = 0;
        self.role = ConnectionRole::Offline;
        self.players.clear();
        self.pending_blocks.clear();
        self.block_versions.clear();
        self.snapshot_seq = 0;
        self.status = "未连接".to_string();
    }

    /// Reset transport and remote-avatar bookkeeping while keeping the
    /// resource alive across menu -> game transitions.
    pub fn reset(&mut self) {
        self.stop_transport();
        self.remote_entities.clear();
    }
}

#[derive(Clone)]
struct ServerClient {
    id: u64,
    name: String,
    last_seen: Instant,
    state: Option<NetPlayer>,
    block_window: Instant,
    block_count: usize,
}

fn clean_name(name: &str) -> String {
    let cleaned: String = name.chars().filter(|c| !c.is_control()).take(24).collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        "探险家".to_string()
    } else {
        cleaned.to_string()
    }
}

fn clean_chat(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect::<String>()
        .trim()
        .to_string()
}

fn resolve_addr(address: &str) -> Result<SocketAddr, String> {
    address
        .to_socket_addrs()
        .map_err(|e| format!("地址无效：{e}"))?
        .next()
        .ok_or_else(|| "地址无效".to_string())
}

fn encode<T: Serialize>(packet: &T) -> Option<Vec<u8>> {
    let bytes = serde_json::to_vec(packet).ok()?;
    (bytes.len() <= MAX_PACKET).then_some(bytes)
}

fn send_to(socket: &UdpSocket, addr: SocketAddr, packet: &ServerPacket) {
    if let Some(bytes) = encode(packet) {
        let _ = socket.send_to(&bytes, addr);
    }
}

fn broadcast(
    socket: &UdpSocket,
    clients: &HashMap<SocketAddr, ServerClient>,
    packet: &ServerPacket,
) {
    let Some(bytes) = encode(packet) else { return };
    for addr in clients.keys() {
        let _ = socket.send_to(&bytes, addr);
    }
}

fn unique_name(clients: &HashMap<SocketAddr, ServerClient>, requested: &str) -> String {
    let base = clean_name(requested);
    let used: HashSet<&str> = clients.values().map(|c| c.name.as_str()).collect();
    if !used.contains(base.as_str()) {
        return base;
    }
    for suffix in 2..=99 {
        let candidate = format!("{base} #{suffix}");
        if !used.contains(candidate.as_str()) {
            return candidate;
        }
    }
    format!("{base} #{}", clients.len() + 1)
}

fn valid_player(player: &NetPlayer) -> bool {
    player.mode <= 3
        && player.planet < 1024
        && player.yaw.is_finite()
        && player
            .pos
            .iter()
            .all(|v| v.is_finite() && v.abs() <= 1_000_000.0)
}

fn valid_block(update: &BlockUpdate) -> bool {
    update.planet < 1024
        && update.x.unsigned_abs() <= 1_000_000
        && update.z.unsigned_abs() <= 1_000_000
        && (0..crate::data::WORLD_H).contains(&update.y)
        && crate::data::BLOCKS
            .iter()
            .any(|block| block.id == update.id)
        && update.dir <= 3
}

fn start_server(address: SocketAddr, host_name: String) -> Result<Sender<()>, String> {
    let socket = UdpSocket::bind(address).map_err(|e| format!("无法监听 {address}：{e}"))?;
    socket
        .set_nonblocking(true)
        .map_err(|e| format!("无法启动服务器：{e}"))?;
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    thread::Builder::new()
        .name("starforge-net-host".to_string())
        .spawn(move || {
            let mut clients: HashMap<SocketAddr, ServerClient> = HashMap::new();
            let mut world_id: Option<u64> = None;
            let mut block_log: HashMap<(u32, usize, i32, i32, i32), BlockUpdate> = HashMap::new();
            let mut next_id = 1u64;
            let mut block_seq = 0u64;
            let mut snapshot_seq = 0u64;
            let mut last_snapshot = Instant::now();
            let mut buf = [0u8; MAX_PACKET];
            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }
                loop {
                    let (len, addr) = match socket.recv_from(&mut buf) {
                        Ok(value) => value,
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(_) => break,
                    };
                    let Ok(packet) = serde_json::from_slice::<ClientPacket>(&buf[..len]) else {
                        continue;
                    };
                    match packet {
                        ClientPacket::Hello {
                            version,
                            name,
                            world_id: requested_world,
                        } => {
                            if version != PROTOCOL_VERSION {
                                send_to(
                                    &socket,
                                    addr,
                                    &ServerPacket::Reject {
                                        reason: "联机协议版本不一致".to_string(),
                                    },
                                );
                                continue;
                            }
                            if let Some(active_world) = world_id {
                                if active_world != requested_world {
                                    send_to(
                                        &socket,
                                        addr,
                                        &ServerPacket::Reject {
                                            reason: "本地世界与主机不一致，请读取相同世界"
                                                .to_string(),
                                        },
                                    );
                                    continue;
                                }
                            } else {
                                world_id = Some(requested_world);
                            }
                            if !clients.contains_key(&addr) && clients.len() >= MAX_PLAYERS {
                                send_to(
                                    &socket,
                                    addr,
                                    &ServerPacket::Reject {
                                        reason: "服务器已满".to_string(),
                                    },
                                );
                                continue;
                            }
                            let final_name = unique_name(&clients, &name);
                            let id = clients.get(&addr).map(|c| c.id).unwrap_or_else(|| {
                                let id = next_id;
                                next_id = next_id.wrapping_add(1).max(1);
                                id
                            });
                            clients.insert(
                                addr,
                                ServerClient {
                                    id,
                                    name: final_name.clone(),
                                    last_seen: Instant::now(),
                                    state: None,
                                    block_window: Instant::now(),
                                    block_count: 0,
                                },
                            );
                            send_to(
                                &socket,
                                addr,
                                &ServerPacket::Welcome {
                                    id,
                                    host_name: host_name.clone(),
                                },
                            );
                            let delta: Vec<BlockUpdate> = block_log.values().cloned().collect();
                            for chunk in delta.chunks(400) {
                                send_to(
                                    &socket,
                                    addr,
                                    &ServerPacket::WorldDelta {
                                        blocks: chunk.to_vec(),
                                    },
                                );
                            }
                            broadcast(
                                &socket,
                                &clients,
                                &ServerPacket::Notice {
                                    text: format!("✦ {final_name} 加入了游戏"),
                                },
                            );
                        }
                        ClientPacket::Ping => {
                            if let Some(client) = clients.get_mut(&addr) {
                                client.last_seen = Instant::now();
                                send_to(&socket, addr, &ServerPacket::Pong);
                            }
                        }
                        ClientPacket::State { mut player } => {
                            let Some(client) = clients.get_mut(&addr) else {
                                continue;
                            };
                            if !valid_player(&player) {
                                continue;
                            }
                            if client
                                .state
                                .as_ref()
                                .is_some_and(|old| player.seq <= old.seq)
                            {
                                continue;
                            }
                            client.last_seen = Instant::now();
                            player.id = client.id;
                            player.name = client.name.clone();
                            client.state = Some(player);
                        }
                        ClientPacket::Chat { text } => {
                            let name = {
                                let Some(client) = clients.get_mut(&addr) else {
                                    continue;
                                };
                                client.last_seen = Instant::now();
                                client.name.clone()
                            };
                            let text = clean_chat(&text);
                            if !text.is_empty() {
                                broadcast(&socket, &clients, &ServerPacket::Chat { name, text });
                            }
                        }
                        ClientPacket::Block { mut update } => {
                            let Some(client) = clients.get_mut(&addr) else {
                                continue;
                            };
                            client.last_seen = Instant::now();
                            if !valid_block(&update) {
                                continue;
                            }
                            if client.block_window.elapsed() >= Duration::from_secs(1) {
                                client.block_window = Instant::now();
                                client.block_count = 0;
                            }
                            if client.block_count >= MAX_BLOCKS_PER_SECOND {
                                continue;
                            }
                            client.block_count += 1;
                            let key = (update.galaxy, update.planet, update.x, update.y, update.z);
                            if !block_log.contains_key(&key) && block_log.len() >= MAX_BLOCK_LOG {
                                continue;
                            }
                            block_seq = block_seq.wrapping_add(1).max(1);
                            update.seq = block_seq;
                            block_log.insert(key, update.clone());
                            broadcast(&socket, &clients, &ServerPacket::Block { update });
                        }
                        ClientPacket::Disconnect => {
                            if let Some(client) = clients.remove(&addr) {
                                broadcast(
                                    &socket,
                                    &clients,
                                    &ServerPacket::Notice {
                                        text: format!("{} 离开了游戏", client.name),
                                    },
                                );
                            }
                        }
                    }
                }

                let now = Instant::now();
                let expired: Vec<SocketAddr> = clients
                    .iter()
                    .filter_map(|(addr, client)| {
                        (now.duration_since(client.last_seen) > CLIENT_TIMEOUT).then_some(*addr)
                    })
                    .collect();
                for addr in expired {
                    clients.remove(&addr);
                }
                if now.duration_since(last_snapshot) >= Duration::from_millis(100) {
                    last_snapshot = now;
                    snapshot_seq = snapshot_seq.wrapping_add(1).max(1);
                    let players = clients.values().filter_map(|c| c.state.clone()).collect();
                    broadcast(
                        &socket,
                        &clients,
                        &ServerPacket::Snapshot {
                            seq: snapshot_seq,
                            players,
                        },
                    );
                }
                thread::sleep(Duration::from_millis(4));
            }
        })
        .map_err(|e| format!("无法启动服务器线程：{e}"))?;
    Ok(stop_tx)
}

fn start_client(
    server: SocketAddr,
    name: String,
    world_id: u64,
) -> Result<(Sender<ClientCommand>, Receiver<ClientEvent>), String> {
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|e| format!("无法创建联机套接字：{e}"))?;
    socket
        .connect(server)
        .map_err(|e| format!("无法连接 {server}：{e}"))?;
    socket
        .set_nonblocking(true)
        .map_err(|e| format!("无法配置联机套接字：{e}"))?;
    let (command_tx, command_rx) = mpsc::channel::<ClientCommand>();
    let (event_tx, event_rx) = mpsc::channel::<ClientEvent>();
    thread::Builder::new()
        .name("starforge-net-client".to_string())
        .spawn(move || {
            let hello = ClientPacket::Hello {
                version: PROTOCOL_VERSION,
                name,
                world_id,
            };
            if let Some(bytes) = encode(&hello) {
                let _ = socket.send(&bytes);
            }
            let mut buf = [0u8; MAX_PACKET];
            let mut last_receive = Instant::now();
            let mut last_ping = Instant::now();
            loop {
                loop {
                    match command_rx.try_recv() {
                        Ok(ClientCommand::Packet(packet)) => {
                            if let Some(bytes) = encode(&packet) {
                                let _ = socket.send(&bytes);
                            }
                        }
                        Ok(ClientCommand::Stop) | Err(TryRecvError::Disconnected) => return,
                        Err(TryRecvError::Empty) => break,
                    }
                }
                loop {
                    match socket.recv(&mut buf) {
                        Ok(len) => {
                            last_receive = Instant::now();
                            if let Ok(packet) = serde_json::from_slice::<ServerPacket>(&buf[..len])
                            {
                                let _ = event_tx.send(ClientEvent::Packet(packet));
                            }
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                        Err(e) => {
                            let _ = event_tx.send(ClientEvent::Error(format!("联机错误：{e}")));
                            return;
                        }
                    }
                }
                if last_ping.elapsed() >= Duration::from_secs(2) {
                    last_ping = Instant::now();
                    if let Some(bytes) = encode(&ClientPacket::Ping) {
                        let _ = socket.send(&bytes);
                    }
                }
                if last_receive.elapsed() > CLIENT_TIMEOUT {
                    let _ = event_tx.send(ClientEvent::Error("连接超时".to_string()));
                    return;
                }
                thread::sleep(Duration::from_millis(4));
            }
        })
        .map_err(|e| format!("无法启动联机线程：{e}"))?;
    Ok((command_tx, event_rx))
}

fn begin_connection(
    net: &mut NetworkState,
    role: ConnectionRole,
    address: SocketAddr,
    world_id: u64,
) -> Result<(), String> {
    net.stop_transport();
    if role == ConnectionRole::Host {
        net.server_stop = Some(start_server(address, clean_name(&net.name))?);
    }
    let server = if role == ConnectionRole::Host {
        let ip = if address.ip().is_unspecified() {
            "127.0.0.1".parse().expect("valid loopback")
        } else {
            address.ip()
        };
        SocketAddr::new(ip, address.port())
    } else {
        address
    };
    match start_client(server, clean_name(&net.name), world_id) {
        Ok((command, events)) => {
            net.command = Some(command);
            net.events = Some(Mutex::new(events));
            net.role = role;
            net.status = format!("正在连接 {server}…");
            net.chat.push(format!("正在连接 Bevy 主机 {server}"));
            Ok(())
        }
        Err(e) => {
            if let Some(stop) = net.server_stop.take() {
                let _ = stop.send(());
            }
            Err(e)
        }
    }
}

fn mode_code(mode: FlightMode) -> u8 {
    match mode {
        FlightMode::Planet | FlightMode::Seated => 0,
        FlightMode::Atmo | FlightMode::AtmoLand => 1,
        FlightMode::Space | FlightMode::Warping => 2,
        FlightMode::Station => 3,
    }
}

fn should_show_remote(
    remote: &RemoteAvatar,
    local_mode: u8,
    local_galaxy: u32,
    local_planet: usize,
) -> bool {
    if remote.galaxy != local_galaxy {
        return false;
    }
    match local_mode {
        0 | 1 => remote.planet == local_planet && remote.mode == local_mode,
        2 => remote.mode == 2,
        3 => remote.mode == 3,
        _ => false,
    }
}

fn apply_block(
    update: &BlockUpdate,
    world: &mut World,
    commands: &mut Commands,
    machines: &Query<(Entity, &Machine)>,
) {
    if !valid_block(update) {
        return;
    }
    world.set(update.x, update.y, update.z, update.id);
    let pos = [update.x, update.y, update.z];
    let block = crate::data::block_by_id(update.id);
    let existing = machines.iter().find(|(_, machine)| machine.pos == pos);
    let existing_matches = existing.is_some_and(|(_, machine)| {
        block.machine.is_some() && machine.kind.block_key() == block.key
    });
    if !existing_matches {
        if let Some((entity, _)) = existing {
            commands.entity(entity).despawn();
        }
        if block.machine.is_some() {
            crate::factory::spawn_machine(commands, pos, block.key, update.dir);
        }
    }
}

fn queue_pending_block(net: &mut NetworkState, update: BlockUpdate) {
    if !valid_block(&update) {
        return;
    }
    let version_key = (update.galaxy, update.planet, update.x, update.y, update.z);
    if update.seq == 0
        || net
            .block_versions
            .get(&version_key)
            .is_some_and(|known| update.seq <= *known)
    {
        return;
    }
    if !net.block_versions.contains_key(&version_key) && net.block_versions.len() >= MAX_BLOCK_LOG {
        return;
    }
    net.block_versions.insert(version_key, update.seq);
    if let Some(existing) = net
        .pending_blocks
        .entry((update.galaxy, update.planet))
        .or_default()
        .iter_mut()
        .find(|pending| pending.x == update.x && pending.y == update.y && pending.z == update.z)
    {
        // Several UDP updates for one cell can arrive before the next Bevy
        // frame. Only the newest state matters; applying every intermediate
        // replacement against a deferred Commands queue can spawn duplicate
        // logical machine entities.
        *existing = update;
        return;
    }
    let pending = net.pending_blocks.values().map(Vec::len).sum::<usize>();
    if pending >= MAX_PENDING_BLOCKS {
        // Drop the oldest available batch entry. UDP is lossy by design; a
        // bounded client is preferable to an unbounded memory queue.
        if let Some((_, updates)) = net
            .pending_blocks
            .iter_mut()
            .find(|(_, updates)| !updates.is_empty())
        {
            updates.remove(0);
        }
    }
    net.pending_blocks
        .entry((update.galaxy, update.planet))
        .or_default()
        .push(update);
}

fn clear_remote_entities(net: &mut NetworkState, commands: &mut Commands) {
    for entity in net.remote_entities.drain().map(|(_, entity)| entity) {
        commands.entity(entity).despawn();
    }
}

#[allow(clippy::too_many_arguments)]
pub fn network_system(
    time: Res<Time>,
    mut net: ResMut<NetworkState>,
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    player: Query<&Player>,
    ship: Res<ShipState>,
    mode: Res<FlightMode>,
    game: Res<SpaceGame>,
    mut world: ResMut<World>,
    machines: Query<(Entity, &Machine)>,
    mut avatars: Query<(&mut Transform, &mut Visibility, &mut RemoteAvatar)>,
    mut block_events: MessageReader<BlockChanged>,
) {
    let events: Vec<ClientEvent> = net
        .events
        .as_ref()
        .and_then(|rx| rx.lock().ok().map(|rx| rx.try_iter().collect()))
        .unwrap_or_default();
    let mut latest_players: Option<Vec<NetPlayer>> = None;
    for event in events {
        match event {
            ClientEvent::Error(error) => {
                clear_remote_entities(&mut net, &mut commands);
                net.stop_transport();
                net.status = error.clone();
                net.chat.push(format!("⚠ {error}"));
                net.connected = false;
            }
            ClientEvent::Packet(packet) => match packet {
                ServerPacket::Welcome { id, host_name } => {
                    let host_name = clean_name(&host_name);
                    net.my_id = id;
                    net.connected = true;
                    net.status = format!("{} · P{id} · 主机 {host_name}", net.role.label());
                    net.chat.push(format!("已连接到 {host_name}"));
                }
                ServerPacket::Reject { reason } => {
                    let reason = clean_chat(&reason);
                    clear_remote_entities(&mut net, &mut commands);
                    net.stop_transport();
                    net.status = format!("连接被拒绝：{reason}");
                    let status = net.status.clone();
                    net.chat.push(status);
                    net.connected = false;
                }
                ServerPacket::Snapshot { seq, players } => {
                    if seq == 0 || seq <= net.snapshot_seq {
                        continue;
                    }
                    net.snapshot_seq = seq;
                    let mut ids = HashSet::new();
                    latest_players = Some(
                        players
                            .into_iter()
                            .filter(valid_player)
                            .filter(|player| player.id != 0 && ids.insert(player.id))
                            .take(MAX_PLAYERS)
                            .collect(),
                    );
                }
                ServerPacket::Chat { name, text } => {
                    let text = clean_chat(&text);
                    if !text.is_empty() {
                        net.chat.push(format!("{}：{text}", clean_name(&name)));
                    }
                }
                ServerPacket::Notice { text } => {
                    let text = clean_chat(&text);
                    if !text.is_empty() {
                        net.chat.push(text);
                    }
                }
                ServerPacket::Block { update } => {
                    queue_pending_block(&mut net, update);
                }
                ServerPacket::WorldDelta { blocks } => {
                    for update in blocks {
                        queue_pending_block(&mut net, update);
                    }
                }
                ServerPacket::Pong => {}
            },
        }
    }
    while net.chat.len() > 80 {
        net.chat.remove(0);
    }

    if let Some(players) = latest_players {
        let active_ids: HashSet<u64> = players.iter().map(|p| p.id).collect();
        let stale: Vec<u64> = net
            .remote_entities
            .keys()
            .copied()
            .filter(|id| !active_ids.contains(id))
            .collect();
        for id in stale {
            if let Some(entity) = net.remote_entities.remove(&id) {
                commands.entity(entity).despawn();
            }
        }
        for remote in &players {
            if remote.id == net.my_id {
                continue;
            }
            let entity = if let Some(entity) = net.remote_entities.get(&remote.id).copied() {
                entity
            } else {
                let appearance = crate::save::Appearance::random(remote.id as u32);
                let parts = crate::char::spawn_humanoid(
                    &mut commands,
                    &asset_server,
                    &appearance,
                    Vec3::from(remote.pos),
                    remote.yaw,
                );
                commands.entity(parts.root).insert(RemoteAvatar {
                    id: remote.id,
                    target: Vec3::from(remote.pos),
                    yaw: remote.yaw,
                    mode: remote.mode,
                    galaxy: remote.galaxy,
                    planet: remote.planet,
                });
                net.remote_entities.insert(remote.id, parts.root);
                parts.root
            };
            if let Ok((_tf, _vis, mut avatar)) = avatars.get_mut(entity) {
                avatar.target = Vec3::from(remote.pos);
                avatar.yaw = remote.yaw;
                avatar.mode = remote.mode;
                avatar.galaxy = remote.galaxy;
                avatar.planet = remote.planet;
            }
        }
        net.players = players;
    }

    let local_mode = mode_code(*mode);
    let current_planet = game.current_planet;
    let smooth = 1.0 - (-12.0 * time.delta_secs()).exp();
    for (mut transform, mut visibility, avatar) in &mut avatars {
        *visibility = if should_show_remote(&avatar, local_mode, game.galaxy.seed, current_planet) {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        transform.translation = transform.translation.lerp(avatar.target, smooth);
        transform.rotation = transform
            .rotation
            .slerp(Quat::from_rotation_y(avatar.yaw), smooth);
    }

    if let Some(updates) = net
        .pending_blocks
        .remove(&(game.galaxy.seed, current_planet))
    {
        for update in updates {
            apply_block(&update, &mut world, &mut commands, &machines);
        }
    }

    for changed in block_events.read() {
        if net.connected {
            net.send(ClientPacket::Block {
                update: BlockUpdate {
                    seq: 0,
                    galaxy: game.galaxy.seed,
                    planet: current_planet,
                    x: changed.x,
                    y: changed.y,
                    z: changed.z,
                    id: changed.id,
                    dir: changed.dir,
                },
            });
        }
    }

    if !net.connected {
        return;
    }
    net.send_acc += time.delta_secs();
    if net.send_acc >= 0.1 {
        net.send_acc = 0.0;
        net.seq = net.seq.wrapping_add(1);
        let Ok(player) = player.single() else { return };
        let pos = match *mode {
            FlightMode::Atmo | FlightMode::AtmoLand | FlightMode::Space | FlightMode::Warping => {
                ship.pos
            }
            _ => player.pos,
        };
        net.send(ClientPacket::State {
            player: NetPlayer {
                id: net.my_id,
                name: clean_name(&net.name),
                pos: pos.to_array(),
                yaw: if local_mode == 0 || local_mode == 3 {
                    player.yaw
                } else {
                    ship.yaw
                },
                mode: local_mode,
                galaxy: game.galaxy.seed,
                planet: current_planet,
                seq: net.seq,
            },
        });
    }
}

pub fn network_ui_system(
    mut contexts: EguiContexts,
    mut ui_state: ResMut<UiState>,
    mut net: ResMut<NetworkState>,
    world: Res<World>,
    game: Res<SpaceGame>,
    mut commands: Commands,
) {
    if ui_state.panel != Panel::Network {
        return;
    }
    let Ok(ctx) = contexts.ctx_mut() else { return };
    if !crate::ui::egui_fonts_ready(ctx) {
        return;
    }
    let world_id = ((world.seed as u64) << 32) | game.galaxy.seed as u64;
    enum Action {
        None,
        Host,
        Join,
        Disconnect,
        SendChat(String),
    }
    let mut action = Action::None;
    let mut open = true;
    egui::Window::new("◉ Bevy 原生联机")
        .open(&mut open)
        .default_width(470.0)
        .default_height(460.0)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(&net.status).color(if net.connected {
                    egui::Color32::from_rgb(0x7d, 0xff, 0x8a)
                } else {
                    egui::Color32::LIGHT_GRAY
                }),
            );
            if !net.connected && net.command.is_none() {
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label("角色名");
                    ui.text_edit_singleline(&mut net.name);
                });
                ui.horizontal(|ui| {
                    ui.label("地址");
                    ui.text_edit_singleline(&mut net.address);
                });
                ui.small("主机填写监听地址（推荐 0.0.0.0:17889）；成员填写主机 IP:端口。双方需读取相同世界。");
                ui.horizontal(|ui| {
                    if ui.button("创建主机").clicked() {
                        action = Action::Host;
                    }
                    if ui.button("加入主机").clicked() {
                        action = Action::Join;
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label(format!("身份：{}", net.role.label()));
                    ui.label(format!("在线：{}", net.players.len()));
                    if ui.button("断开").clicked() {
                        action = Action::Disconnect;
                    }
                });
                ui.collapsing("在线玩家", |ui| {
                    for player in &net.players {
                        ui.label(format!(
                            "P{} · {} · {}",
                            player.id,
                            player.name,
                            ["地表", "大气层", "太空", "空间站"]
                                .get(player.mode as usize)
                                .copied()
                                .unwrap_or("未知")
                        ));
                    }
                });
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .max_height(240.0)
                .show(ui, |ui| {
                    for line in &net.chat {
                        ui.label(line);
                    }
                });
            if net.connected {
                let response = ui.add(
                    egui::TextEdit::singleline(&mut net.chat_input)
                        .hint_text("输入聊天内容，Enter 发送"),
                );
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (enter || ui.button("发送").clicked()) && !net.chat_input.trim().is_empty() {
                    action = Action::SendChat(std::mem::take(&mut net.chat_input));
                }
            }
        });
    if !open {
        ui_state.panel = Panel::None;
    }
    match action {
        Action::None => {}
        Action::Host => match resolve_addr(&net.address) {
            Ok(addr) => {
                if let Err(e) = begin_connection(&mut net, ConnectionRole::Host, addr, world_id) {
                    net.status = e;
                }
            }
            Err(e) => net.status = e,
        },
        Action::Join => match resolve_addr(&net.address) {
            Ok(addr) => {
                if let Err(e) = begin_connection(&mut net, ConnectionRole::Client, addr, world_id) {
                    net.status = e;
                }
            }
            Err(e) => net.status = e,
        },
        Action::Disconnect => {
            let entities: Vec<Entity> = net
                .remote_entities
                .drain()
                .map(|(_, entity)| entity)
                .collect();
            net.stop_transport();
            for entity in entities {
                commands.entity(entity).despawn();
            }
        }
        Action::SendChat(text) => net.send(ClientPacket::Chat { text }),
    }
}

pub fn disconnect_system(mut net: ResMut<NetworkState>) {
    net.reset();
}

/// Multiplayer plugin: UDP host/client relay, remote avatars and the panel UI.
pub struct NetworkPlugin;

impl Plugin for NetworkPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<BlockChanged>()
            .init_resource::<NetworkState>()
            .add_systems(
                Update,
                network_system
                    .in_set(crate::schedule::GameSet::CommonNetwork)
                    .run_if(in_state(crate::schedule::GameState::Playing)),
            )
            .add_systems(
                Update,
                network_ui_system
                    .in_set(crate::schedule::GameSet::SaveNetworkUi)
                    .run_if(in_state(crate::schedule::GameState::Playing)),
            )
            .add_systems(
                OnExit(crate::schedule::GameState::Playing),
                disconnect_system,
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_round_trip() {
        let packet = ClientPacket::Block {
            update: BlockUpdate {
                seq: 0,
                galaxy: 77,
                planet: 2,
                x: -4,
                y: 42,
                z: 9,
                id: 3,
                dir: 1,
            },
        };
        let bytes = serde_json::to_vec(&packet).unwrap();
        let back: ClientPacket = serde_json::from_slice(&bytes).unwrap();
        match back {
            ClientPacket::Block { update } => {
                assert_eq!(update.planet, 2);
                assert_eq!((update.x, update.y, update.z, update.id), (-4, 42, 9, 3));
            }
            _ => panic!("wrong packet"),
        }
    }

    #[test]
    fn sanitizers_and_validation_are_bounded() {
        assert_eq!(clean_name("  A\0B  "), "AB");
        assert_eq!(clean_chat(" hi\r\nthere "), "hithere");
        assert!(!valid_block(&BlockUpdate {
            seq: 1,
            galaxy: 77,
            planet: 0,
            x: 0,
            y: crate::data::WORLD_H,
            z: 0,
            id: 1,
            dir: 0,
        }));
        assert!(!valid_block(&BlockUpdate {
            seq: 1,
            galaxy: 77,
            planet: 0,
            x: i32::MIN,
            y: 1,
            z: 0,
            id: 1,
            dir: 0,
        }));
        assert!(!valid_block(&BlockUpdate {
            seq: 1,
            galaxy: 77,
            planet: 0,
            x: 0,
            y: 1,
            z: 0,
            id: u8::MAX,
            dir: 0,
        }));
    }

    #[test]
    fn pending_blocks_coalesce_repeated_cells() {
        let mut net = NetworkState::default();
        for (seq, id) in [crate::data::ids::STONE, crate::data::ids::AIR]
            .into_iter()
            .enumerate()
        {
            queue_pending_block(
                &mut net,
                BlockUpdate {
                    seq: seq as u64 + 1,
                    galaxy: 77,
                    planet: 2,
                    x: 3,
                    y: 4,
                    z: 5,
                    id,
                    dir: 0,
                },
            );
        }
        let pending = &net.pending_blocks[&(77, 2)];
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, crate::data::ids::AIR);
    }

    #[test]
    fn pending_blocks_ignore_late_older_udp_update() {
        let mut net = NetworkState::default();
        for (seq, id) in [(9, crate::data::ids::AIR), (8, crate::data::ids::STONE)] {
            queue_pending_block(
                &mut net,
                BlockUpdate {
                    seq,
                    galaxy: 77,
                    planet: 2,
                    x: 3,
                    y: 4,
                    z: 5,
                    id,
                    dir: 0,
                },
            );
        }
        let pending = &net.pending_blocks[&(77, 2)];
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].seq, 9);
        assert_eq!(pending[0].id, crate::data::ids::AIR);
    }

    #[test]
    fn remote_visibility_is_isolated_by_galaxy() {
        let remote = RemoteAvatar {
            id: 2,
            target: Vec3::ZERO,
            yaw: 0.0,
            mode: 0,
            galaxy: 77,
            planet: 0,
        };
        assert!(should_show_remote(&remote, 0, 77, 0));
        assert!(!should_show_remote(&remote, 0, 78, 0));
    }

    #[test]
    fn native_host_relays_state_chat_and_blocks() {
        let probe = UdpSocket::bind("127.0.0.1:0").unwrap();
        let addr = probe.local_addr().unwrap();
        drop(probe);
        let stop = start_server(addr, "测试主机".to_string()).unwrap();
        let (c1, e1) = start_client(addr, "甲".to_string(), 77).unwrap();
        let (c2, e2) = start_client(addr, "乙".to_string(), 77).unwrap();

        let wait_for = |rx: &Receiver<ClientEvent>, pred: &dyn Fn(&ServerPacket) -> bool| {
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline {
                match rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(ClientEvent::Packet(packet)) if pred(&packet) => return Some(packet),
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            None
        };
        assert!(wait_for(&e1, &|p| matches!(p, ServerPacket::Welcome { .. })).is_some());
        assert!(wait_for(&e2, &|p| matches!(p, ServerPacket::Welcome { .. })).is_some());

        for (tx, name, seq) in [(&c1, "甲", 1), (&c2, "乙", 1)] {
            tx.send(ClientCommand::Packet(ClientPacket::State {
                player: NetPlayer {
                    id: 0,
                    name: name.to_string(),
                    pos: [1.0, 42.0, 3.0],
                    yaw: 0.25,
                    mode: 0,
                    galaxy: 77,
                    planet: 0,
                    seq,
                },
            }))
            .unwrap();
        }
        assert!(
            wait_for(&e1, &|p| {
                matches!(p, ServerPacket::Snapshot { players, .. } if players.len() == 2)
            })
            .is_some()
        );

        c1.send(ClientCommand::Packet(ClientPacket::Chat {
            text: "你好".to_string(),
        }))
        .unwrap();
        assert!(
            wait_for(&e2, &|p| {
                matches!(p, ServerPacket::Chat { name, text } if name == "甲" && text == "你好")
            })
            .is_some()
        );

        c1.send(ClientCommand::Packet(ClientPacket::Block {
            update: BlockUpdate {
                seq: 0,
                galaxy: 77,
                planet: 0,
                x: 2,
                y: 40,
                z: 3,
                id: 4,
                dir: 1,
            },
        }))
        .unwrap();
        assert!(
            wait_for(&e2, &|p| {
                matches!(p, ServerPacket::Block { update } if update.x == 2 && update.id == 4)
            })
            .is_some()
        );

        let _ = c1.send(ClientCommand::Stop);
        let _ = c2.send(ClientCommand::Stop);
        let _ = stop.send(());
    }
}

// ---------- Plugin ----------

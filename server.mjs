/* ============================================================
   STARFORGE - server.mjs
   跨平台联机服务器（Windows / macOS / Linux，Node.js ≥ 18，零依赖）

   功能：
    · HTTP 静态文件服务器（默认 :17888）——好友浏览器直接打开即载入游戏
    · WebSocket 游戏服务器（默认 :17889）——房间 / 聊天 / 传送 / 时间同步
    · 世界持久化：服务器拥有世界（种子/方块改动/机器/市场/标记/星系），
      主机下线世界依然存在（类 Minecraft 专用服务器）
    · 玩家数据持久化：每个玩家的背包/科技/外观/位置随服务器保存
    · 配置：server-config.json（首次运行自动生成）
    · 控制台：Ctrl+C 优雅退出（先存档再关服）

   启动：
     node server.mjs
     node server.mjs --port-ws 17900 --name "我的服务器" --password 123
   ============================================================ */
import http from 'node:http';
import crypto from 'node:crypto';
import fs from 'node:fs';
import fsp from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

// ------------------------------------------------------------
// 配置
// ------------------------------------------------------------
const DEFAULTS = {
  httpPort: 17888,
  wsPort: 17889,
  serverName: 'STARFORGE 服务器',
  motd: '欢迎来到星穹熔炉',
  password: '',
  maxPlayers: 8,
  saveDir: 'save',
  autosaveSec: 30,
};

function arg(name, fallback){
  for (let i = 2; i < process.argv.length; i++){
    const a = process.argv[i];
    if (a === '--' + name) return process.argv[i + 1] ?? fallback;
    if (a.startsWith('--' + name + '=')) return a.slice(('--' + name + '=').length);
  }
  return fallback;
}

function loadConfig(){
  const cfg = { ...DEFAULTS };
  const file = path.join(__dirname, 'server-config.json');
  try {
    const raw = fs.readFileSync(file, 'utf8');
    const obj = JSON.parse(raw);
    for (const k of Object.keys(DEFAULTS)){
      if (obj[k] !== undefined) cfg[k] = obj[k];
    }
  } catch (e){
    if (e.code !== 'ENOENT'){
      console.warn(`[!] server-config.json 解析失败，使用默认配置（${e.message}）`);
    } else {
      // 首次运行：生成默认配置，方便玩家修改
      try { fs.writeFileSync(file, JSON.stringify(DEFAULTS, null, 2) + '\n', 'utf8'); }
      catch (err){ /* 只读目录等场景忽略 */ }
    }
  }
  // 命令行覆盖
  if (arg('port-http', null) !== null) cfg.httpPort = Number(arg('port-http')) || cfg.httpPort;
  if (arg('port-ws', null) !== null) cfg.wsPort = Number(arg('port-ws')) || cfg.wsPort;
  if (arg('name', null) !== null) cfg.serverName = String(arg('name'));
  if (arg('motd', null) !== null) cfg.motd = String(arg('motd'));
  if (arg('password', null) !== null) cfg.password = String(arg('password'));
  if (arg('max-players', null) !== null) cfg.maxPlayers = Math.max(1, Number(arg('max-players')) || cfg.maxPlayers);
  if (arg('save-dir', null) !== null) cfg.saveDir = String(arg('save-dir'));
  if (arg('autosave', null) !== null) cfg.autosaveSec = Math.max(2, Number(arg('autosave')) || cfg.autosaveSec);
  cfg.reset = process.argv.includes('--reset');
  if (!path.isAbsolute(cfg.saveDir)) cfg.saveDir = path.join(__dirname, cfg.saveDir);
  return cfg;
}
const CFG = loadConfig();
if (CFG.reset){
  try { fs.rmSync(path.join(CFG.saveDir, 'world.json'), { force: true }); } catch(e){}
  console.log('[reset] 服务器世界已重置');
}

// ------------------------------------------------------------
// 常量（与客户端 world.js 保持一致）
// ------------------------------------------------------------
const CHUNK = 16, WORLD_H = 96, CHUNK_CELLS = CHUNK * CHUNK * WORLD_H;
const DAY_LEN = 480;                       // 秒/天（与 main.js 一致）
const WS_GUID = '258EAFA5-E914-47DA-95CA-C5AB0DC85B11';
const MAX_FRAME = 64 * 1024 * 1024;        // 单帧上限 64MB（世界上传包可能很大）
const MAX_CHAT = 200;

// ------------------------------------------------------------
// RLE（与客户端 world.js serialize 完全一致的格式：[run, value] 成对）
// ------------------------------------------------------------
function rleEncode(data){
  const out = [];
  let cur = data[0], run = 1;
  for (let i = 1; i < data.length; i++){
    if (data[i] === cur && run < 65535) run++;
    else { out.push(run, cur); cur = data[i]; run = 1; }
  }
  out.push(run, cur);
  return out;
}
function rleDecode(arr){
  const data = new Array(CHUNK_CELLS);
  let i = 0;
  for (let p = 0; p < arr.length; p += 2){
    const run = arr[p], val = arr[p + 1];
    for (let r = 0; r < run; r++) data[i++] = val;
  }
  return data;
}
const voxelIdx = (x, y, z) => ((y * CHUNK) + (z - Math.floor(z / CHUNK) * CHUNK)) * CHUNK + (x - Math.floor(x / CHUNK) * CHUNK);
const chunkKey = (x, z) => Math.floor(x / CHUNK) + ',' + Math.floor(z / CHUNK);

function validRle(arr){
  if (!Array.isArray(arr) || arr.length % 2 !== 0) return false;
  let total = 0;
  for (let i = 0; i < arr.length; i += 2){
    const run = arr[i], val = arr[i + 1];
    if (!Number.isInteger(run) || run < 1 || run > 65535) return false;
    if (!Number.isInteger(val) || val < 0 || val > 255) return false;
    total += run;
  }
  return total === CHUNK_CELLS;
}

// ------------------------------------------------------------
// 世界存储
// ------------------------------------------------------------
let world = null;        // 见 loadWorld 注释
let worldDirty = false;
let saveTimer = null;

function freshWorld(){
  return {
    v: 4,
    name: '未命名世界', creative: false, dropMult: 4,
    galaxySeed: 0, galaxyCount: 1, currentPlanet: 0,
    dayTime: 0.3, dayAt: Date.now(),
    planets: {},          // pid -> {mods:{key:rle}, machines:[], shipPos:[x,y,z], seed, biome}
    galaxyArchives: {},   // 星系种子 -> 客户端序列化数据
    market: {},           // 商品 -> 价格
    mapMarks: {},         // pid -> [{x,z,y,label,gal}]
    flags: {},            // 世界事件旗标
    warpLock: null,
    players: {},          // 玩家名 -> { char, pos:{planet,p,st,yaw} }
    hostKey: '',          // 主机所有权密钥：首个上传世界的客户端签发，后续声明主机必须出示
  };
}
function dayTimeNow(){
  if (!world) return 0.3;
  return (world.dayTime + (Date.now() - world.dayAt) / 1000 / DAY_LEN) % 1;
}
function worldFilePath(){ return path.join(CFG.saveDir, 'world.json'); }

function loadWorld(){
  try {
    const raw = fs.readFileSync(worldFilePath(), 'utf8');
    const w = JSON.parse(raw);
    // 存档版本：v2/v3（旧版）与 v4（当前）均可读取；缺省字段按当前格式补齐后统一升为 v4
    if (!w || typeof w !== 'object' || ![2, 3, 4].includes(w.v) || typeof w.planets !== 'object'){
      throw new Error('世界存档格式无效');
    }
    const base = freshWorld();
    for (const k of ['galaxyArchives', 'market', 'mapMarks', 'flags']) if (w[k] === undefined) w[k] = base[k];
    w.terrainV = Number.isFinite(w.terrainV) ? Math.max(1, w.terrainV | 0) : 1;
    w.warpLock = w.warpLock && typeof w.warpLock === 'object' ? w.warpLock : null;
    w.players = w.players && typeof w.players === 'object' ? w.players : {};
    w.hostKey = (typeof w.hostKey === 'string' && w.hostKey.length > 0 && w.hostKey.length <= 64) ? w.hostKey : '';
    // 坏档防护：逐星球校验区块 RLE 与机器记录，丢弃损坏条目（防 rleDecode 越界/异常数据注入）
    let dropped = 0;
    for (const [pid, p] of Object.entries(w.planets)){
      if (!p || typeof p !== 'object'){ delete w.planets[pid]; dropped++; continue; }
      if (p.mods && typeof p.mods === 'object'){
        for (const [key, rle] of Object.entries(p.mods)){
          if (!validRle(rle)){ delete p.mods[key]; dropped++; }
        }
      } else p.mods = {};
      if (Array.isArray(p.machines)){
        p.machines = p.machines.slice(0, 20000).filter(m =>
          m && Number.isInteger(m.x) && Number.isInteger(m.y) && Number.isInteger(m.z)
          && typeof m.type === 'string' && m.type.length >= 1 && m.type.length <= 20
        ).map(m => ({ x: m.x, y: m.y, z: m.z, type: m.type, dir: Number.isInteger(m.dir) ? m.dir : 0, data: (m.data && typeof m.data === 'object') ? m.data : {} }));
      } else p.machines = [];
      if (!Array.isArray(p.shipPos) || p.shipPos.length < 3 || !p.shipPos.every(Number.isFinite)) p.shipPos = [0, 40, 0];
      if (!Number.isFinite(p.seed)) p.seed = 0;
      if (typeof p.biome !== 'string') p.biome = 'green';
    }
    if (dropped) console.warn(`[world] 存档损坏条目 ${dropped} 个，已丢弃`);
    if (!Object.keys(w.planets).length) w.planets['0'] = { mods: {}, machines: [], shipPos: [0, 40, 0], seed: 0, biome: 'green' };
    w.v = 4;
    w.dayAt = Date.now();
    world = w;
    console.log(`[world] 已载入世界「${w.name}」（${Object.keys(w.planets).length} 颗已访星球）`);
  } catch (e){
    if (e.code !== 'ENOENT') console.warn(`[!] 世界存档读取失败：${e.message}`);
    world = null;
  }
}
function scheduleSave(){
  worldDirty = true;
  if (saveTimer) return;
  saveTimer = setTimeout(() => { saveTimer = null; saveWorld(); }, 2000);
}
// 玩家档案修剪：world.players 按名字无限累积会胀大 world.json —— 仅保留最近活跃的 200 名
function prunePlayers(){
  if (!world || !world.players) return;
  const names = Object.keys(world.players);
  if (names.length <= 200) return;
  names.sort((a, b) => (world.players[a].lastSeen || 0) - (world.players[b].lastSeen || 0));
  for (let i = 0; i < names.length - 200; i++) delete world.players[names[i]];
}
// 存档写入串行化：防 debounce 与周期自动存档并发竞争同一临时文件
let saveChain = Promise.resolve();
async function saveWorld(){
  if (!world) return;
  worldDirty = false;
  prunePlayers();
  const snapshot = JSON.stringify(world);   // 立即快照，避免串行期间世界继续变化导致写旧值
  saveChain = saveChain.then(async () => {
    try {
      await fsp.mkdir(CFG.saveDir, { recursive: true });
      const tmp = worldFilePath() + '.tmp';
      await fsp.writeFile(tmp, snapshot, 'utf8');
      try {
        await fsp.rename(tmp, worldFilePath());
      } catch (re){
        // Windows 上目标文件被短暂占用时 rename 可能失败 → 直接覆写兜底
        await fsp.writeFile(worldFilePath(), snapshot, 'utf8');
        await fsp.rm(tmp, { force: true });
      }
    } catch (e){
      console.error(`[!] 世界保存失败：${e.message}`);
    }
  });
  return saveChain;
}
// 定期自动存档（worldDirty 且到达间隔）
setInterval(() => {
  if (world && worldDirty){
    saveWorld().catch(() => {});
  }
}, Math.max(2, CFG.autosaveSec) * 1000);

// ------------------------------------------------------------
// 世界操作（服务器权威：方块 / 机器 / 市场 / 标记）
// ------------------------------------------------------------
function planetOf(pid){ return world.planets[pid] || (world.planets[pid] = { mods: {}, machines: [], shipPos: [0, 40, 0], seed: 0, biome: 'green' }); }

function applyBlk(pid, x, y, z, b, full){
  if (!Number.isInteger(x) || !Number.isInteger(y) || !Number.isInteger(z)) return false;
  if (y < 0 || y >= WORLD_H) return false;
  if (!Number.isInteger(b) || b < 0 || b > 255) return false;
  const p = planetOf(pid);
  const key = chunkKey(x, z);
  let arr = p.mods[key];
  if (!arr){
    if (!validRle(full)) return false;     // 未知区块必须携带整块 RLE
    p.mods[key] = full.slice();
    arr = p.mods[key];
  }
  const data = rleDecode(arr);
  data[voxelIdx(x, y, z)] = b;
  p.mods[key] = rleEncode(data);
  return true;
}

// 机器 data 规模上限：恶意客户端可注入巨型对象图拖垮广播/存档
function capData(d){
  if (!d || typeof d !== 'object') return {};
  const s = JSON.stringify(d).length;
  return s <= 65536 ? d : {};
}
function applyMac(pid, m){
  const p = planetOf(pid);
  const at = p.machines.findIndex(s => s.x === m.x && s.y === m.y && s.z === m.z);
  if (m.op === 'add'){
    if (!Number.isInteger(m.x) || !Number.isInteger(m.y) || !Number.isInteger(m.z)) return null;
    if (typeof m.type !== 'string' || m.type.length > 20 || m.type.length < 1) return null;
    if (at < 0 && p.machines.length >= 20000) return null;   // 每星球机器规模上限
    const rec = { x: m.x, y: m.y, z: m.z, type: m.type, dir: Number.isInteger(m.dir) ? m.dir : 0, data: capData(m.data) };
    if (at >= 0) p.machines[at] = rec; else p.machines.push(rec);
    return rec;
  } else if (m.op === 'remove'){
    if (at >= 0) p.machines.splice(at, 1);
    else return null;
    return { x: m.x, y: m.y, z: m.z };
  }
  return null;
}
function applyMacData(pid, arr){
  if (!Array.isArray(arr) || arr.length > 4096) return false;
  const p = planetOf(pid);
  for (const d of arr){
    if (!d || !Number.isInteger(d.x) || !Number.isInteger(d.y) || !Number.isInteger(d.z)) continue;
    const at = p.machines.findIndex(s => s.x === d.x && s.y === d.y && s.z === d.z);
    if (at >= 0) p.machines[at].data = capData(d.data);
  }
  return true;
}

// ------------------------------------------------------------
// 消息广播 / 客户端
// ------------------------------------------------------------
const clients = new Map();     // id -> client
let nextId = 1;

function broadcast(msg, except){
  const text = JSON.stringify(msg);
  for (const c of clients.values()){
    if (c === except) continue;
    sendText(c, text);
  }
}
function sendText(c, text){
  if (c.dead) return;
  const payload = Buffer.from(text, 'utf8');
  const len = payload.length;
  let header;
  if (len < 126){
    header = Buffer.alloc(2);
    header[0] = 0x81; header[1] = len;
  } else if (len < 65536){
    header = Buffer.alloc(4);
    header[0] = 0x81; header[1] = 126;
    header.writeUInt16BE(len, 2);
  } else {
    header = Buffer.alloc(10);
    header[0] = 0x81; header[1] = 127;
    header.writeBigUInt64BE(BigInt(len), 2);
  }
  try {
    c.socket.write(Buffer.concat([header, payload]));
  } catch (e){ c.dead = true; }
}
function sys(text, except){
  broadcast({ t: 'chat', sys: 1, text }, except);
}
function kick(c, reason){
  if (c.dead) return;
  sendText(c, JSON.stringify({ t: 'ws-err', reason }));
  setTimeout(() => { try { c.socket.destroy(); } catch(e){} }, 120);
  c.dead = true;
}

// 令牌桶限速
function makeBucket(cap, perSec){
  return { cap, perSec, tokens: cap, last: Date.now() };
}
function take(b, n = 1){
  const now = Date.now();
  b.tokens = Math.min(b.cap, b.tokens + (now - b.last) / 1000 * b.perSec);
  b.last = now;
  if (b.tokens < n) return false;
  b.tokens -= n;
  return true;
}

function playerList(){
  return [...clients.values()].map(c => ({ id: c.id, name: c.name }));
}
function byName(name){ for (const c of clients.values()){ if (c.name === name) return c; } return null; }

// ---------- 主机所有权（防伪造 role:'host' 覆盖/重置世界）----------
function onlineHost(){
  for (const c of clients.values()) if (c.role === 'host') return c;
  return null;
}
function canClaimHost(key){
  if (onlineHost()) return false;              // 已有在线主机：主机席位不重复
  if (!world || !world.hostKey) return true;   // 尚无世界 / 未签发密钥：先到先得
  return typeof key === 'string' && key.length > 0 && key === world.hostKey;
}
function issueHostKey(){
  if (!world) return null;
  if (!world.hostKey) world.hostKey = crypto.randomBytes(16).toString('hex');
  return world.hostKey;
}

function makeClient(socket){
  return {
    id: nextId++, socket, name: '', app: null, role: 'guest',
    dead: false, buf: Buffer.alloc(0), frag: [], fragOp: 0, fragSize: 0,
    lastTraffic: Date.now(), lastPong: Date.now(),
    lastPlanet: -1, lastSt: '', lastPos: null, lastYaw: 0,
    charT: 0, lastCharAt: 0,
    buckets: {
      blk: makeBucket(40, 20), mac: makeBucket(20, 10), macdata: makeBucket(4, 1),
      cre: makeBucket(4, 2), chat: makeBucket(5, 1), char: makeBucket(2, 0.1),
      market: makeBucket(2, 0.5), world: makeBucket(2, 0.1), tp: makeBucket(3, 1),
      pos: makeBucket(40, 20),   // 位置：客户端 10/s，突发 40 帧后限到 20/s
    },
  };
}

// ------------------------------------------------------------
// 协议处理
// ------------------------------------------------------------
function sanitizeName(raw){
  let s = String(raw || '').replace(/[\u0000-\u001f\u007f]/g, '').trim().slice(0, 16);
  if (!s) s = '旅行者';
  // 重名追加序号
  if (byName(s)){
    let i = 2;
    while (byName(s + '#' + i)) i++;
    s = s + '#' + i;
  }
  return s;
}

function worldPackage(){
  // 发给玩家的世界快照（不含其他玩家的私有数据）
  return {
    v: 4, name: world.name, creative: !!world.creative, dropMult: world.dropMult || 4,
    terrainV: world.terrainV || 1,   // 地形生成器版本（联机两端一致性）
    galaxySeed: world.galaxySeed | 0, galaxyCount: world.galaxyCount || 1,
    currentPlanet: world.currentPlanet | 0,
    dayTime: dayTimeNow(),
    planets: world.planets, galaxyArchives: world.galaxyArchives,
    market: world.market, mapMarks: world.mapMarks, flags: world.flags, warpLock: world.warpLock,
  };
}
function sendInit(c){
  if (!world) return;
  const me = world.players[c.name] || {};
  sendText(c, JSON.stringify({
    t: 'init',
    world: worldPackage(),
    you: { id: c.id, name: c.name, char: me.char || null },
    spawn: me.pos || null,
  }));
}

// 兽群存档清洗：herds=[cx,cz,idx,x×10,z×10,hp,homeX×10,homeZ×10]，removed=['cx,cz',mask]
function sanitizeCreatures(raw){
  if (!raw || typeof raw !== 'object') return null;
  const out = { herds: [], removed: [] };
  if (Array.isArray(raw.herds)){
    for (const e of raw.herds.slice(0, 8192)){
      if (!Array.isArray(e) || e.length < 8) continue;
      if (!Number.isInteger(e[0]) || !Number.isInteger(e[1]) || !Number.isInteger(e[2])) continue;
      const ok = e.slice(3, 8).every(Number.isFinite);
      if (!ok) continue;
      out.herds.push([e[0], e[1], e[2], Math.round(e[3]), Math.round(e[4]), Math.max(0, Math.round(e[5])), Math.round(e[6]), Math.round(e[7])]);
    }
  }
  if (Array.isArray(raw.removed)){
    for (const e of raw.removed.slice(0, 4096)){
      if (!Array.isArray(e) || e.length < 2 || typeof e[0] !== 'string' || !/^-?\d+,-?\d+$/.test(e[0]) || !Number.isInteger(e[1])) continue;
      out.removed.push([e[0], e[1] | 0]);
    }
  }
  return (out.herds.length || out.removed.length) ? out : null;
}

function sanitizeUpload(raw){
  if (!raw || typeof raw !== 'object') return null;
  const w = freshWorld();
  w.name = String(raw.name || '未命名世界').slice(0, 32);
  w.creative = !!raw.creative;
  w.dropMult = [1, 4, 7].includes(raw.dropMult) ? raw.dropMult : 4;
  w.galaxySeed = Number.isFinite(raw.galaxySeed) ? (raw.galaxySeed | 0) : 0;
  w.terrainV = Number.isFinite(raw.terrainV) ? Math.max(1, raw.terrainV | 0) : 1;
  w.galaxyCount = Number.isFinite(raw.galaxyCount) ? Math.max(1, raw.galaxyCount | 0) : 1;
  w.currentPlanet = Number.isInteger(raw.currentPlanet) && raw.currentPlanet >= 0 && raw.currentPlanet < 32 ? raw.currentPlanet : 0;
  w.dayTime = Number.isFinite(raw.dayTime) ? Math.min(1, Math.max(0, raw.dayTime)) : 0.3;
  w.dayAt = Date.now();
  if (raw.planets && typeof raw.planets === 'object'){
    let chunks = 0;
    for (const [pid, p] of Object.entries(raw.planets)){
      if (!p || typeof p !== 'object') continue;
      if (chunks > 4096) break;    // 世界规模上限保护
      const out = { mods: {}, machines: [], shipPos: [0, 40, 0], seed: 0, biome: 'green' };
      if (p.mods && typeof p.mods === 'object'){
        for (const [key, rle] of Object.entries(p.mods)){
          if (!/^-?\d+,-?\d+$/.test(key)) continue;
          if (!validRle(rle)) continue;
          out.mods[key] = rle;
          chunks++;
          if (chunks > 4096) break;
        }
      }
      if (Array.isArray(p.machines)){
        for (const m of p.machines.slice(0, 5000)){
          if (!m || !Number.isInteger(m.x) || !Number.isInteger(m.y) || !Number.isInteger(m.z)) continue;
          if (typeof m.type !== 'string' || m.type.length > 20 || m.type.length < 1) continue;
          out.machines.push({ x: m.x, y: m.y, z: m.z, type: m.type, dir: Number.isInteger(m.dir) ? m.dir : 0, data: (m.data && typeof m.data === 'object') ? m.data : {} });
        }
      }
      if (Array.isArray(p.shipPos) && p.shipPos.length >= 3 && p.shipPos.every(Number.isFinite)) out.shipPos = p.shipPos.slice(0, 3);
      if (Number.isFinite(p.seed)) out.seed = p.seed | 0;
      if (typeof p.biome === 'string') out.biome = p.biome.slice(0, 24);
      const cr = sanitizeCreatures(p.creatures);
      if (cr) out.creatures = cr;
      w.planets[pid] = out;
    }
  }
  if (raw.galaxyArchives && typeof raw.galaxyArchives === 'object') w.galaxyArchives = raw.galaxyArchives;
  if (raw.market && typeof raw.market === 'object'){
    for (const [k, v] of Object.entries(raw.market)){
      if (Number.isFinite(v)) w.market[k] = Math.min(2, Math.max(0, v));
    }
  }
  if (raw.mapMarks && typeof raw.mapMarks === 'object') w.mapMarks = sanitizeMarks(raw.mapMarks);
  if (raw.flags && typeof raw.flags === 'object') w.flags = raw.flags;
  if (raw.warpLock && typeof raw.warpLock === 'object') w.warpLock = raw.warpLock;
  if (!Object.keys(w.planets).length) w.planets['0'] = { mods: {}, machines: [], shipPos: [0, 40, 0], seed: 0, biome: 'green' };
  return w;
}

function handleMessage(c, m){
  if (!m || typeof m !== 'object' || typeof m.t !== 'string') return;
  // 认证门槛：完成 hello（含密码/版本校验）前，仅允许握手与探活消息。
  // 此前任何原始连接都能在未认证状态下改世界/广播/传送（密码形同虚设）。
  if (!c.named && m.t !== 'hello' && m.t !== 'ping') return;
  switch (m.t){
    case 'hello': {
      if (c.named) return;
      if (CFG.password && String(m.password || '') !== CFG.password){ kick(c, 'auth'); return; }
      if (m.v !== 3){ kick(c, 'version'); return; }
      c.named = true;
      c.name = sanitizeName(m.name);
      c.role = 'guest';
      if (m.role === 'host'){
        // 主机席位：已有在线主机或密钥不符时降级为成员（防伪造主机身份覆写/重置世界）
        if (canClaimHost(m.hostKey)) c.role = 'host';
        else sendText(c, JSON.stringify({ t: 'chat', sys: 1, text: '已有主机在线（或主机密钥不符），你以成员身份加入' }));
      }
      if (m.app && typeof m.app === 'object' && JSON.stringify(m.app).length < 800) c.app = m.app;
      // 回送服务器信息（role = 服务器裁定后的实际角色；players = 当前在线名单，晚加入也能看到先到者）
      sendText(c, JSON.stringify({
        t: 'ws-id', id: c.id, svName: CFG.serverName, motd: CFG.motd,
        role: c.role,
        players: playerList(), hasWorld: !!world,
        worldName: world ? world.name : null,
        worldTime: world ? dayTimeNow() : null,
        auth: 'ok',
      }));
      if (world){
        sendInit(c);
      } else {
        sendText(c, JSON.stringify({ t: 'world-missing' }));
      }
      broadcast({ t: 'joined', id: c.id, name: c.name, app: c.app }, c);
      sys(`✦ ${c.name} 加入了游戏`);
      console.log(`[+] ${c.name}（P${c.id}，${c.role}）已连接，在线 ${clients.size}`);
      break;
    }
    case 'ping':
      sendText(c, JSON.stringify({ t: 'pong' }));
      break;
    case 'chat': {
      if (!take(c.buckets.chat)) break;
      const text = String(m.text || '').replace(/[\u0000-\u001f\u007f]/g, '').trim().slice(0, MAX_CHAT);
      if (!text) break;
      if (text === '/help'){
        sendText(c, JSON.stringify({ t: 'chat', sys: 1, text: '服务器命令：/help 帮助 · /list 在线玩家 · 玩家列表面板可一键传送到队友身边' }));
      } else if (text === '/list'){
        sendText(c, JSON.stringify({ t: 'chat', sys: 1, text: '在线玩家：' + playerList().map(p => p.name).join('、') }));
      } else {
        broadcast({ t: 'chat', id: c.id, name: c.name, text });
        console.log(`[chat] ${c.name}: ${text}`);
      }
      break;
    }
    case 'pos': {
      // 位置中继：服务器记录最后位置（传送/重生用），转发给其他玩家
      if (!take(c.buckets.pos)) return;   // 高频消息唯一无上限路径 → 令牌桶限速（客户端 10/s，突发 40）
      // 严格三元素坐标：拒绝超长数组（中继放大：任意长度的 p 原样广播给所有在线玩家）
      if (!Array.isArray(m.p) || m.p.length !== 3 || !m.p.every(v => Number.isFinite(v) && Math.abs(v) <= 1e6) || !Number.isFinite(m.yaw)) return;
      if (Number.isInteger(m.planet)) c.lastPlanet = m.planet;
      c.lastSt = typeof m.st === 'string' ? m.st.slice(0, 24) : '';
      c.lastPos = { planet: c.lastPlanet, p: m.p.slice(0, 3), st: c.lastSt, yaw: m.yaw };
      c.lastYaw = m.yaw;
      if (world && c.name){
        world.players[c.name] = world.players[c.name] || {};
        world.players[c.name].pos = c.lastPos;
        world.players[c.name].lastSeen = Date.now();
      }
      // 优化：外观只在 hello/外观变化时发（app 字段仅透传已有消息，不额外包装）
      const out = { t: 'pos', id: c.id, planet: c.lastPlanet, st: c.lastSt, p: m.p.slice(0, 3), yaw: m.yaw };
      if (m.app){
        const appJson = JSON.stringify(m.app);
        if (appJson.length <= 800) out.app = m.app;   // 外观上限与 hello 一致，防超大对象中继放大
      }
      if (Number.isFinite(m.act)) out.act = m.act;   // 动作位仅限数值（客户端只发 0/1）
      broadcast(out, c);
      break;
    }
    case 'blk': {
      if (!take(c.buckets.blk)) return;
      if (!world || !Number.isInteger(m.planet)) return;
      if (!applyBlk(m.planet, m.x, m.y, m.z, m.b, m.full)) return;
      scheduleSave();
      broadcast({ t: 'blk', id: c.id, planet: m.planet, x: m.x, y: m.y, z: m.z, b: m.b }, c);
      break;
    }
    case 'mac': {
      if (!take(c.buckets.mac)) return;
      if (!world || !Number.isInteger(m.planet)) return;
      const rec = applyMac(m.planet, m);   // 返回清洗后的记录，广播用清洗值而非原始消息
      if (!rec) return;
      scheduleSave();
      broadcast({ t: 'mac', id: c.id, planet: m.planet, op: m.op, x: rec.x, y: rec.y, z: rec.z, type: rec.type, dir: rec.dir, data: rec.data }, c);
      break;
    }
    case 'mac-data': {
      if (!take(c.buckets.macdata)) return;
      if (!world || !Number.isInteger(m.planet)) return;
      if (!applyMacData(m.planet, m.arr)) return;
      // 低频率：合并保存节流（广播清洗后的 arr，避免脏数据注入其他客户端）
      scheduleSave();
      const clean = (Array.isArray(m.arr) ? m.arr.slice(0, 4096) : [])
        .filter(d => d && Number.isInteger(d.x) && Number.isInteger(d.y) && Number.isInteger(d.z))
        .map(d => ({ x: d.x, y: d.y, z: d.z, data: capData(d.data) }));
      broadcast({ t: 'mac-data', id: c.id, planet: m.planet, arr: clean }, c);
      break;
    }
    case 'market': {
      if (!take(c.buckets.market)) return;
      if (!world || !m.market || typeof m.market !== 'object') return;
      for (const [k, v] of Object.entries(m.market)){
        if (Number.isFinite(v)) world.market[k] = Math.min(2, Math.max(0, v));
      }
      scheduleSave();
      broadcast({ t: 'market', market: world.market }, c);
      break;
    }
    case 'mapMarks': {
      if (!take(c.buckets.market)) return;
      if (!world || typeof m.mapMarks !== 'object') return;
      if (m.pid !== undefined && Array.isArray(m.arr)){
        // 单星球形式：替换该星球的标记
        world.mapMarks[m.pid] = sanitizeMarks({ [m.pid]: m.arr })[m.pid] || [];
      } else {
        // 全量形式：整体替换（客户端 5 秒差异同步路径）
        world.mapMarks = sanitizeMarks(m.mapMarks);
      }
      scheduleSave();
      broadcast({ t: 'mapMarks', mapMarks: world.mapMarks }, c);
      break;
    }
    case 'cre':
    case 'cre-hit':
    case 'cre-kill': {
      if (!take(c.buckets.cre)) return;
      // 生物同步：直接中继（含发送者 id 与星球）；先做基础校验防脏数据注入
      if (m.t === 'cre' && (!Array.isArray(m.arr) || m.arr.length > 2048 ||
          !m.arr.every(e => Array.isArray(e) && e.length <= 12 && e.every(v => Number.isFinite(v))))) return;
      broadcast({ t: m.t, id: c.id, planet: m.planet, arr: m.arr, cid: m.cid, dmg: m.dmg }, c);
      break;
    }
    case 'char': {
      if (!take(c.buckets.char)) return;
      if (!world || !c.name) return;
      if (!m.char || typeof m.char !== 'object') return;
      const size = JSON.stringify(m.char).length;
      if (size > 512 * 1024) return;
      world.players[c.name] = world.players[c.name] || {};
      world.players[c.name].char = m.char;
      world.players[c.name].lastSeen = Date.now();
      c.lastCharAt = Date.now();
      scheduleSave();
      break;
    }
    case 'tp': {
      if (!take(c.buckets.tp)) return;
      const target = clients.get(Number(m.target));
      if (!target || !target.lastPos){
        sendText(c, JSON.stringify({ t: 'chat', sys: 1, text: '目标玩家不存在或尚未进入世界' }));
        return;
      }
      sendText(c, JSON.stringify({ t: 'tp-you', planet: target.lastPos.planet, p: target.lastPos.p, st: target.lastPos.st, yaw: target.lastYaw, target: target.name }));
      sendText(target, JSON.stringify({ t: 'chat', sys: 1, text: `${c.name} 传送到你身边` }));
      console.log(`[tp] ${c.name} → ${target.name}`);
      break;
    }
    case 'world-upload': {
      if (c.role !== 'host') return;
      if (!take(c.buckets.world)){ sendText(c, JSON.stringify({ t: 'chat', sys: 1, text: '上传过于频繁，请稍候' })); return; }
      const w = sanitizeUpload(m.world);
      if (!w){ sendText(c, JSON.stringify({ t: 'chat', sys: 1, text: '世界数据无效，上传被拒绝' })); return; }
      const prevKey = world ? world.hostKey : '';
      world = w;
      world.hostKey = prevKey || issueHostKey();   // 首次上传签发所有权密钥（旧密钥随世界保留）
      worldDirty = true;
      saveWorld();
      if (!prevKey) sendText(c, JSON.stringify({ t: 'host-key', key: world.hostKey }));
      console.log(`[world] ${c.name} 上传了世界「${w.name}」（${Object.keys(w.planets).length} 颗星球）`);
      sys(`✦ 主机 ${c.name} 上传了世界「${w.name}」，正在为所有玩家重建…`, c);
      for (const other of clients.values()){
        if (other !== c) sendInit(other);
      }
      break;
    }
    case 'reset-world': {
      if (c.role !== 'host') return;
      world = null;   // 密钥随世界一并清除：重置后下一位主机重新签发
      try { fs.rmSync(worldFilePath(), { force: true }); } catch(e){}
      console.log(`${ts()} [world] ${c.name} 重置了服务器世界`);
      broadcast({ t: 'world-missing' });
      sendText(c, JSON.stringify({ t: 'chat', sys: 1, text: '服务器世界已重置，开始游戏后会自动上传你的世界' }));
      break;
    }
    default:
      break;
  }
}

// ------------------------------------------------------------
// WebSocket 帧编解码
// ------------------------------------------------------------
function parseFrames(c, data){
  c.buf = Buffer.concat([c.buf, data]);
  while (true){
    const buf = c.buf;
    if (buf.length < 2) return;
    const fin = (buf[0] & 0x80) !== 0;
    const opcode = buf[0] & 0x0f;
    const masked = (buf[1] & 0x80) !== 0;
    let len = buf[1] & 0x7f;
    let off = 2;
    if (len === 126){
      if (buf.length < 4) return;
      len = buf.readUInt16BE(2); off = 4;
    } else if (len === 127){
      if (buf.length < 10) return;
      const big = buf.readBigUInt64BE(2);
      if (big > BigInt(MAX_FRAME)){ c.dead = true; return; }
      len = Number(big); off = 10;
    }
    if (len > MAX_FRAME){ c.dead = true; return; }
    const maskLen = masked ? 4 : 0;
    if (buf.length < off + maskLen + len) return;
    let payload;
    if (masked){
      const mask = buf.subarray(off, off + 4);
      payload = Buffer.alloc(len);
      for (let i = 0; i < len; i++) payload[i] = buf[off + 4 + i] ^ mask[i & 3];
    } else {
      payload = buf.subarray(off, off + len);
    }
    c.buf = buf.subarray(off + maskLen + len);

    if (opcode === 8){ // close
      if (fin){
        try { c.socket.end(Buffer.from([0x88, 0x00])); } catch(e){}
        c.dead = true;
      }
      return;
    }
    if (opcode === 9){ // ping → pong
      const pong = Buffer.alloc(2 + payload.length);
      pong[0] = 0x8A; pong[1] = payload.length;
      payload.copy(pong, 2);
      try { c.socket.write(pong); } catch(e){ c.dead = true; }
      continue;
    }
    if (opcode === 10){ // pong
      c.lastPong = Date.now();
      continue;
    }
    // 数据帧
    if (opcode === 0){ // continuation
      if (c.fragOp === 0){ continue; } // 无头分片：丢弃
      c.fragSize += payload.length;
      if (c.fragSize > MAX_FRAME){ c.dead = true; return; }   // 逐片累计上限：无头续帧洪水不再可无限吞内存
      c.frag.push(payload);
      if (fin){
        const full = Buffer.concat(c.frag);
        c.frag = []; c.fragOp = 0; c.fragSize = 0;
        handleText(c, full.toString('utf8'));
      }
    } else if (opcode === 1){
      c.frag = [];
      c.fragSize = 0;
      if (fin){
        handleText(c, payload.toString('utf8'));
      } else {
        c.fragOp = 1;
        c.fragSize = payload.length;
        c.frag.push(payload);
      }
    } else if (opcode === 2){
      // 二进制帧：不支持，忽略
      if (!fin){ c.fragOp = 0; c.fragSize = 0; }
    }
  }
}
function handleText(c, text){
  c.lastTraffic = Date.now();
  let m;
  try { m = JSON.parse(text); }
  catch (e){ return; }
  handleMessage(c, m);
}

// ------------------------------------------------------------
// HTTP 静态服务器
// ------------------------------------------------------------
const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.glb': 'model/gltf-binary',
  '.wasm': 'application/wasm',
  '.mp3': 'audio/mpeg',
  '.ogg': 'audio/ogg',
  '.wav': 'audio/wav',
  // 注意：故意不含 .json/.txt/.md/.mjs 与无扩展名文件——静态根目录下存在
  // server-config.json（服务器密码）、save/world.json（主机密钥/玩家数据）、.git 等
  // 敏感文件，任何扩展名兜底（octet-stream）都会把它们暴露给任何访客。
};

function sanitizeMarks(pids){
  // pids: {pid: [marks]} → 校验并精简
  const out = {};
  for (const [pid, arr] of Object.entries(pids)){
    if (!Array.isArray(arr)) continue;
    out[pid] = arr.slice(0, 256)
      .filter(m => m && Number.isFinite(m.x) && Number.isFinite(m.z) && Number.isFinite(m.y))
      .map(m => ({ x: m.x, z: m.z, y: m.y, label: String(m.label || '标记').slice(0, 12), gal: !!m.gal }));
  }
  return out;
}

function serveHttp(req, res){
  try {
    const url = new URL(req.url, 'http://localhost');
    let pathname = decodeURIComponent(url.pathname);
    if (pathname === '/__status'){
      res.writeHead(200, { 'Content-Type': 'application/json; charset=utf-8', 'Cache-Control': 'no-cache', 'Access-Control-Allow-Origin': '*' });
      res.end(JSON.stringify({
        ok: true, name: CFG.serverName, motd: CFG.motd,
        wsPort: CFG.wsPort, httpPort: CFG.httpPort,
        hasWorld: !!world, worldName: world ? world.name : null,
        dayTime: world ? dayTimeNow() : null,
        players: playerList().map(p => p.name),
        maxPlayers: CFG.maxPlayers,
        uptimeSec: Math.round(process.uptime()),
      }));
      return;
    }
    if (pathname === '/') pathname = '/index.html';
    const rel = pathname.replace(/^\/+/, '');
    const ERR = { 'Access-Control-Allow-Origin': '*' };   // 错误/公开资源保持可跨域读取状态码；敏感内容绝不返回 200
    // 隐藏文件/目录（.git、.e2e-save 等）一律拒绝
    if (rel.split(/[\\/]+/).some(seg => seg.startsWith('.'))){
      res.writeHead(403, ERR); res.end('403 Forbidden'); return;
    }
    const full = path.resolve(__dirname, rel);
    const rootBound = __dirname + path.sep;
    if (!((full === __dirname) || full.startsWith(rootBound))){
      res.writeHead(403, ERR); res.end('403 Forbidden'); return;
    }
    // 存档目录整树拒绝（world.json 内含主机密钥与玩家数据）
    const saveBound = path.resolve(CFG.saveDir);
    if (full === saveBound || full.startsWith(saveBound + path.sep)){
      res.writeHead(403, ERR); res.end('403 Forbidden'); return;
    }
    if (!fs.existsSync(full) || !fs.statSync(full).isFile()){
      res.writeHead(404, Object.assign({ 'Content-Type': 'text/plain; charset=utf-8' }, ERR));
      res.end('404 Not Found');
      return;
    }
    const ext = path.extname(full).toLowerCase();
    const ct = MIME[ext];
    if (!ct){
      // 白名单外的扩展名（.json/.md/.mjs/无扩展名等）一律 404，不提供 octet-stream 兜底
      res.writeHead(404, Object.assign({ 'Content-Type': 'text/plain; charset=utf-8' }, ERR));
      res.end('404 Not Found');
      return;
    }
    const cache = ext === '.html' ? 'no-cache' : 'max-age=86400';
    const body = fs.readFileSync(full);
    res.writeHead(200, {
      'Content-Type': ct,
      'Content-Length': body.length,
      'Cache-Control': cache,
      'Connection': 'close',
      'Access-Control-Allow-Origin': '*',
    });
    res.end(body);
  } catch (e){
    try { res.writeHead(500, { 'Access-Control-Allow-Origin': '*' }); res.end('500'); } catch(e2){}
  }
}

// ------------------------------------------------------------
// 启动
// ------------------------------------------------------------
loadWorld();

const httpServer = http.createServer(serveHttp);
httpServer.listen(CFG.httpPort, () => {
  console.log('=============================================');
  console.log(` STARFORGE 联机服务器 · ${CFG.serverName}`);
  console.log('=============================================');
  console.log(` 游戏页面   : http://<本机IP>:${CFG.httpPort}`);
  console.log(` WebSocket  : ws://<本机IP>:${CFG.wsPort}`);
  console.log(` 世界存档   : ${path.join(CFG.saveDir, 'world.json')}`);
  console.log(` 在线上限   : ${CFG.maxPlayers} 人${CFG.password ? ' · 需要密码' : ' · 无密码（局域网信任模式）'}`);
  console.log(` 提示：主机在游戏里点「创建房间」并开始游戏，世界即上传到服务器`);
  console.log(`       好友浏览器打开 http://<本机IP>:${CFG.httpPort} → 输入 IP 加入房间`);
  console.log('=============================================');
  if (world){
    console.log(` [world] 已有世界「${world.name}」：任何玩家加入都会自动进入`);
  } else {
    console.log(' [world] 暂无世界：等待主机创建/上传');
  }
});
httpServer.on('error', e => {
  console.error(`[!] HTTP 端口 ${CFG.httpPort} 绑定失败：${e.message}`);
  process.exit(1);
});

const wsServer = http.createServer((req, res) => { res.writeHead(404); res.end(); });
wsServer.on('upgrade', (req, socket) => {
  const key = req.headers['sec-websocket-key'];
  if (!key || !String(req.headers.upgrade || '').toLowerCase().includes('websocket')){
    socket.destroy();
    return;
  }
  if (clients.size >= CFG.maxPlayers){
    // 房间已满：握手前拒绝
    socket.write('HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n');
    socket.destroy();
    return;
  }
  const accept = crypto.createHash('sha1').update(key + WS_GUID).digest('base64');
  socket.write(
    'HTTP/1.1 101 Switching Protocols\r\n' +
    'Upgrade: websocket\r\n' +
    'Connection: Upgrade\r\n' +
    `Sec-WebSocket-Accept: ${accept}\r\n\r\n`
  );
  socket.setNoDelay(true);
  socket.setTimeout(0);
  const c = makeClient(socket);
  clients.set(c.id, c);
  socket.on('data', chunk => {
    c.lastTraffic = Date.now();
    parseFrames(c, chunk);
    if (c.dead) dropClient(c, 'frame');
  });
  socket.on('error', () => { c.dead = true; dropClient(c, 'socket-error'); });
  socket.on('close', () => { c.dead = true; dropClient(c, 'socket-close'); });
});
wsServer.listen(CFG.wsPort);
wsServer.on('error', e => {
  console.error(`[!] WebSocket 端口 ${CFG.wsPort} 绑定失败：${e.message}`);
  process.exit(1);
});

function ts(){ return new Date().toISOString().slice(17, 23); }
function dropClient(c, reason = '?'){
  if (!clients.has(c.id)) return;
  clients.delete(c.id);
  try { c.socket.destroy(); } catch(e){}   // 彻底断开（心跳剔除/主动踢出也要关掉 TCP）
  if (c.named){
    console.log(`[-] ${c.name}（P${c.id}）离开，在线 ${clients.size}（原因：${reason}）`);
    broadcast({ t: 'left', id: c.id, name: c.name });
    sys(`✧ ${c.name} 离开了游戏`);
  } else {
    console.log(`[-] 未完成握手的连接断开，在线 ${clients.size}`);
  }
}

// 服务器心跳：协议层 ping（浏览器自动回 pong）；静默连接剔除
setInterval(() => {
  const now = Date.now();
  for (const c of clients.values()){
    if (c.dead) continue;
    if (now - c.lastPong > 90 * 1000){ c.dead = true; continue; }
    if (now - c.lastTraffic > 120 * 1000){ c.dead = true; c._reason = 'silent'; continue; }
    if (now - c.lastPong > 15 * 1000 && (!c.lastPingAt || now - c.lastPingAt > 15 * 1000)){
      const ping = Buffer.from([0x89, 0x00]);
      try { c.socket.write(ping); } catch(e){ c.dead = true; }
      c.lastPingAt = now;
    }
  }
  for (const c of [...clients.values()]){
    if (c.dead) dropClient(c, c._reason || 'heartbeat');
  }
  // 时间广播（服务器权威昼夜）：每 2 秒一次，客户端本地外推保持平滑
  if (world && now - lastTimeSendAt > 2000){
    broadcast({ t: 'time', dayTime: dayTimeNow() });
    lastTimeSendAt = now;
    lastTimeBroadcast = dayTimeNow();
  }
}, 1000);
let lastTimeSendAt = 0, lastTimeBroadcast = -1;

// 优雅退出
let shuttingDown = false;
async function shutdown(sig){
  if (shuttingDown) return;
  shuttingDown = true;
  console.log(`\n[!] 收到 ${sig}，正在保存并关闭服务器…`);
  if (world){
    broadcast({ t: 'server-closing' });
    await saveWorld();
  }
  process.exit(0);
}
process.on('SIGINT', () => shutdown('SIGINT'));
process.on('SIGTERM', () => shutdown('SIGTERM'));

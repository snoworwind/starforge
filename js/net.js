/* ============================================================
   STARFORGE - net.js
   跨平台联机客户端（配合 server.mjs，见 start-server.bat/sh）

   协议：WebSocket JSON（服务器权威：世界/昼夜/聊天/传送）
   同步内容：
    · 世界（种子/全星球方块改动/机器/市场/标记/星系档案）——初始化整包 + 增量
    · 玩家（位置/外观/动作 + 人物数据由服务器按名字持久化）
    · 机器运行数据（燃料/进度/皮带物品，2 秒快照）
    · 生物（确定性批次 + 1.2 秒快照对齐 + 击杀广播）
    · 昼夜（服务器每 2 秒广播，本地外推）
    · 聊天 / 玩家列表 / 一键传送
   ============================================================ */
'use strict';

const Net = (() => {
  const WS_PORT = 17889;
  const HTTP_PORT = 17888;
  const DAY_LEN = 480;           // 与 main.js / server.mjs 一致
  const NAME_KEY = 'starforge_net_name';

  let ws = null;
  let role = null;               // null | 'host' | 'guest'
  let myId = 0;                  // 服务器分配
  let myName = '';               // 服务器最终确认的名字
  let connected = false;
  let serverInfo = null;         // {svName, motd, hasWorld, worldName}
  let applyDepth = 0;            // 应用远程操作期间不回播
  function beginApply(){ applyDepth++; }
  function endApply(){ applyDepth--; }
  function isApplying(){ return applyDepth > 0; }
  let patched = false;
  let posTimer = 0, pingT = 0;
  let macSyncT = 0, lastMacData = '';
  let creSyncT = 0;
  let marketSyncT = 0, lastMarket = '';
  let markSyncT = 0, lastMarks = '';
  let charSyncT = 0, lastChar = '';
  let lastAppJson = '';
  let pendingInit = null;        // 加载中收到的世界包：等待就绪后应用
  let waitingWorld = false;      // 服务器世界为空（等待主机上传）
  let dayT = { v: 0.3, at: 0 };  // 服务器时间（本地外推）
  let gotInit = false;

  const remotes = new Map();     // id -> 化身记录（含 name/app/位置插值）
  const players = new Map();     // id -> {name, planet, st}（服务器名单）
  const pendingBlk = {};         // planetId -> [msg]
  const pendingMac = {};         // planetId -> [msg]
  const pendingMacData = {};     // planetId -> [msg]

  let onStatus = () => {};
  let onToast = (title, sub, dur) => { if (window.UI) UI.bigMessage(title, sub, dur); };

  // ================= 连接 / 房间 =================
  function defaultAddr(){
    return (location.protocol === 'http:' || location.protocol === 'https:') ? location.hostname : 'localhost';
  }
  function storedName(){
    try { return localStorage.getItem(NAME_KEY) || ''; } catch(e){ return ''; }
  }
  function hostKeyStoreKey(host, port){
    return 'starforge_net_hostkey_' + String(host || '').trim() + ':' + port;
  }
  function storedHostKey(host, port){
    try { return localStorage.getItem(hostKeyStoreKey(host, port)) || ''; } catch(e){ return ''; }
  }
  function saveHostKey(host, port, key){
    try { localStorage.setItem(hostKeyStoreKey(host, port), String(key || '')); } catch(e){}
  }
  // ---------- 身份令牌（服务器按名字持久化档案；令牌防同名冒领） ----------
  function tokenStoreKey(host, port){
    return 'starforge_net_tokens_' + String(host || '').trim() + ':' + port;
  }
  function storedToken(host, port, name){
    try {
      const o = JSON.parse(localStorage.getItem(tokenStoreKey(host, port)) || '{}');
      return (o && o[name]) || '';
    } catch(e){ return ''; }
  }
  function saveToken(host, port, name, t){
    try {
      const k = tokenStoreKey(host, port);
      const o = JSON.parse(localStorage.getItem(k) || '{}');
      o[name] = t;
      localStorage.setItem(k, JSON.stringify(o));
    } catch(e){}
  }
  function pickName(){
    const el = document.getElementById('netName');
    if (el && el.value.trim()) return el.value.trim();
    const saved = storedName();
    if (saved) return saved;
    if (window.Game && Game.charName && Game.charName !== '旅行者') return Game.charName;
    return saved || '旅行者';
  }
  function password(){
    const el = document.getElementById('netPass');
    return el ? el.value : '';
  }

  function openSocket(url, roleHint){
    return new Promise((res, rej) => {
      let settled = false;
      const fail = err => { if (!settled){ settled = true; try { ws.close(); } catch(e){} rej(err); } };
      const ws2 = new WebSocket(url);
      ws = ws2;
      const to = setTimeout(() => fail(new Error('连接超时：请确认服务器已启动（双击 start-server.bat / 启动联机主机.bat，macOS/Linux 用 start-server.sh）')), 8000);
      ws2.onopen = () => {
        role = roleHint;
        // 主机：出示本机保存的所有权密钥（服务器据此裁定实际角色，防伪造主机身份）
        let u;
        try { u = new URL(url); } catch(e){ u = { hostname: defaultAddr(), port: String(WS_PORT) }; }
        const hello = { t: 'hello', v: 4, name: pickName(), role: roleHint, password: password(), app: (window.Player && Player.appearance) || null };
        if (roleHint === 'host') hello.hostKey = storedHostKey(u.hostname, u.port);
        // 身份令牌：出示本机保存的名字令牌，服务器据此放行同名档案
        hello.token = storedToken(u.hostname, u.port, pickName());
        broadcast(hello);
      };
      ws2.onmessage = e => {
        let m;
        try { m = JSON.parse(e.data); } catch(err){ return; }
        if (m.t === 'ws-id'){
          clearTimeout(to);
          myId = m.id;
          const me = Array.isArray(m.players) ? m.players.find(p => p.id === m.id) : null;
          myName = me && me.name ? me.name : pickName();
          if (m.auth !== 'ok'){ fail(new Error('服务器拒绝：' + (m.auth || '未知'))); return; }
          // 服务器裁定的实际角色（伪造/抢占主机会被降级为成员）
          if (m.role === 'host' || m.role === 'guest') role = m.role;
          // 完整在线名单：晚加入也能看到先到的玩家（含自己）
          if (Array.isArray(m.players)) for (const p of m.players) players.set(p.id, { id: p.id, name: p.name });
          serverInfo = { svName: m.svName, motd: m.motd, hasWorld: m.hasWorld, worldName: m.worldName };
          connected = true;
          ensurePatched();
          // 保存服务器签发的身份令牌（按服务器确认后的名字；重名 #2 去重时同时记在原名下）
          if (m.token){
            let u;
            try { u = new URL(url); saveToken(u.hostname, u.port, myName, m.token); if (myName !== pickName()) saveToken(u.hostname, u.port, pickName(), m.token); } catch(e){}
          }
          try { localStorage.setItem(NAME_KEY, myName.replace(/#\d+$/, '')); } catch(e2){}
          // 房主中途（已在游戏里）创建房间：服务器无世界时立即上传本机世界
          if (role === 'host' && !m.hasWorld && window.Game && gameReady() && Game.buildNetWorld){
            broadcast({ t: 'world-upload', world: Game.buildNetWorld() });
            serverInfo.hasWorld = true;
          }
          onStatus();
          if (!settled){ settled = true; res(myId); }
          return;
        }
        if (m.t === 'host-key'){
          // 服务器签发的主机所有权密钥：按 服务器地址 持久保存，之后重连声明主机时出示
          let u;
          try { u = new URL(url); saveHostKey(u.hostname, u.port, m.key); } catch(e){}
          return;
        }
        if (m.t === 'ws-err'){
          clearTimeout(to);
          const why = m.reason === 'auth' ? '密码错误' : m.reason === 'version' ? '版本不匹配，请 Ctrl+F5 强刷页面' : m.reason === 'full' ? '服务器已满' : m.reason === 'name-taken' ? '该名字在本服务器已有档案：请换一个名字，或使用创建该档案时的浏览器/设备' : '被服务器拒绝';
          fail(new Error(why));
          return;
        }
        onMsg(m);
      };
      ws2.onclose = () => {
        connected = false;
        role = null;
        clearRemotes();
        players.clear();
        onStatus();
        if (!settled){ settled = true; clearTimeout(to); rej(new Error('连接被拒绝：请确认服务器已启动')); }
      };
      ws2.onerror = () => {};
    });
  }

  async function connect(addr, roleHint){
    disconnect();
    let host = (addr || defaultAddr()).trim();
    let port = WS_PORT;
    const m = host.match(/^(.+):(\d+)$/);
    if (m){ host = m[1]; port = Number(m[2]) || WS_PORT; }   // 支持 host:port 自定义端口
    const tryUrl = async (p) => {
      const scheme = location.protocol === 'https:' ? 'wss://' : 'ws://';
      return openSocket(scheme + host + ':' + p, roleHint);
    };
    // 状态探测（https 页面用 https，避免混合内容被浏览器拦截）
    async function probeStatus(){
      try {
        const scheme = location.protocol === 'https:' ? 'https://' : 'http://';
        const r = await fetch(scheme + host + ':' + HTTP_PORT + '/__status', { signal: AbortSignal.timeout(3000) });
        if (!r.ok) return null;
        return await r.json();
      } catch (e){ return null; }
    }
    try {
      return await tryUrl(port);
    } catch (e1){
      // 端口被改过？向 HTTP 状态端点询问 WebSocket 端口后重试一次
      const st = await probeStatus();
      if (st && Number.isFinite(st.wsPort) && st.wsPort !== port){
        try { return await tryUrl(st.wsPort); } catch(e2){}
      }
      // 满员：握手前 503 拒绝，WebSocket 拿不到原因 → 用状态端点给出准确提示
      if (st && Array.isArray(st.players) && Number.isFinite(st.maxPlayers) && st.players.length >= st.maxPlayers){
        throw new Error('服务器已满（' + st.maxPlayers + '/' + st.maxPlayers + '）');
      }
      throw e1;
    }
  }
  async function hostRoom(addr){
    const id = await connect(addr, 'host');
    onStatus();
    return id;
  }
  async function joinRoom(addr){
    const id = await connect(addr, 'guest');
    onStatus();
    return id;
  }
  function disconnect(){
    // 尽力而为：断开前把人物数据交给服务器保存（另有 30 秒周期同步兜底）
    if (connected && window.Game && gameReady() && Game.buildCharData){
      try { broadcast({ t: 'char', id: myId, char: Game.buildCharData() }); } catch(e){}
    }
    if (ws){ try { ws.onclose = null; ws.close(); } catch(e){} }
    ws = null;
    connected = false;
    role = null;
    gotInit = false;
    waitingWorld = false;
    pendingInit = null;
    serverInfo = null;
    clearRemotes();
    players.clear();
    onStatus();
  }
  function broadcast(msg){
    if (ws && ws.readyState === 1) ws.send(JSON.stringify(msg));
  }
  function active(){ return connected; }
  function status(){
    if (!connected) return '未连接';
    const n = players.size;
    const roleTxt = role === 'host' ? '主机' : '成员';
    const sv = serverInfo ? serverInfo.svName : '';
    return `${roleTxt} P${myId} · ${sv} · 在线 ${n} 人` + (waitingWorld ? ' · 等待世界…' : '');
  }

  // ================= 世界包处理 =================
  function onInit(m){
    const w = m.world;
    if (!w || !window.Game){
      if (!w) return;
      pendingInit = m;
      return;
    }
    gotInit = true;
    waitingWorld = false;
    dayT = { v: Number.isFinite(w.dayTime) ? w.dayTime : 0.3, at: performance.now() };
    if (m.you && m.you.name) myName = m.you.name;
    if (window.Game.state === 'loading'){ pendingInit = m; return; }   // 加载中：稍后应用
    applyInit(m);
  }
  function applyInit(m){
    pendingInit = null;
    beginApply();
    Promise.resolve().then(() => Game.joinGame(Object.assign({}, m.world, { spawn: m.spawn, you: m.you })))
      .catch(e => console.warn('[net] init apply', e))
      .finally(endApply);
  }

  // ================= 消息处理 =================
  function onMsg(m){
    switch (m.t){
      case 'init': onInit(m); break;
      case 'world-missing':
        waitingWorld = true;
        onStatus();
        if (role === 'guest') onToast('服务器世界为空', '等待房主创建世界…', 6000);
        break;
      case 'time':
        if (Number.isFinite(m.dayTime)) dayT = { v: m.dayTime % 1, at: performance.now() };
        break;
      case 'pos': onPos(m); break;
      case 'blk': onBlk(m); break;
      case 'mac': onMac(m); break;
      case 'mac-data':
        if (m.id === myId) break;
        if (!Array.isArray(m.arr) || !Number.isInteger(m.planet)) return;
        if (gameReady() && Game.currentPlanet === m.planet){
          beginApply(); Factory.applyData(m.arr); endApply();
        } else {
          (pendingMacData[m.planet] = pendingMacData[m.planet] || []).push(m.arr);
        }
        break;
      case 'cre':
        if (m.id === myId || !Array.isArray(m.arr) || m.planet !== Game.currentPlanet) return;
        if (gameReady()) Creatures.applyRemote(m.arr);
        break;
      case 'cre-kill':
        if (m.id === myId || !Number.isFinite(m.cid) || m.planet !== Game.currentPlanet) return;
        if (gameReady()) Creatures.remoteKill(m.cid);
        break;
      case 'market':
        if (m.market && window.Game && Game.applyMarket) Game.applyMarket(m.market);
        break;
      case 'mapMarks':
        if (m.mapMarks && window.Game && Game.applyMapMarks) Game.applyMapMarks(m.mapMarks);
        break;
      case 'chat': onChat(m); break;
      case 'joined':
        players.set(m.id, { id: m.id, name: m.name });
        onStatus();
        break;
      case 'left':
        removeRemote(m.id);
        players.delete(m.id);
        onStatus();
        break;
      case 'tp-you':
        if (window.Game && Game.tpTo && Array.isArray(m.p) && m.p.length >= 3 && m.p.every(Number.isFinite)){
          Game.tpTo(Number.isInteger(m.planet) ? m.planet : Game.currentPlanet, m.p, m.st, m.target);
        }
        break;
      case 'server-closing':
        onToast('服务器已关闭', '世界已保存，可稍后重连', 6000);
        disconnect();
        break;
    }
  }
  function onBlk(m){
    if (m.id === myId) return;
    if (!Number.isInteger(m.x) || !Number.isInteger(m.y) || !Number.isInteger(m.z) || !Number.isFinite(m.b)) return;
    if (gameReady() && Game.currentPlanet === m.planet){
      beginApply();
      World.set(m.x, m.y, m.z, m.b);
      endApply();
    } else {
      (pendingBlk[m.planet] = pendingBlk[m.planet] || []).push(m);
    }
  }
  function onMac(m){
    if (m.id === myId) return;
    if (!Number.isInteger(m.x) || !Number.isInteger(m.y) || !Number.isInteger(m.z)) return;
    if (m.op === 'add' && (typeof m.type !== 'string' || m.type.length > 20 || !window.BLOCKS)) return;
    if (m.op !== 'add' && m.op !== 'remove') return;
    const bkOf = type => Object.keys(BLOCKS).find(k => BLOCKS[k].machine === type);
    if (gameReady() && Game.currentPlanet === m.planet){
      beginApply();
      if (m.op === 'add'){
        const bk = bkOf(m.type);
        if (bk){
          const mach = Factory.place(m.x, m.y, m.z, bk, m.dir);
          if (mach && m.data && typeof m.data === 'object') mach.data = m.data;
        }
      } else Factory.remove(m.x, m.y, m.z);
      endApply();
    } else {
      (pendingMac[m.planet] = pendingMac[m.planet] || []).push(m);
    }
  }
  function drainPending(){
    if (!gameReady()) return;
    const pid = Game.currentPlanet;
    beginApply();
    if (pendingBlk[pid]){ for (const m of pendingBlk[pid]) World.set(m.x, m.y, m.z, m.b); delete pendingBlk[pid]; }
    if (pendingMac[pid]){
      const bkOf = type => Object.keys(BLOCKS).find(k => BLOCKS[k].machine === type);
      for (const m of pendingMac[pid]){
        if (m.op === 'add'){
          const bk = bkOf(m.type);
          if (bk){
            const mach = Factory.place(m.x, m.y, m.z, bk, m.dir);
            if (mach && m.data && typeof m.data === 'object') mach.data = m.data;
          }
        } else Factory.remove(m.x, m.y, m.z);
      }
      delete pendingMac[pid];
    }
    if (pendingMacData[pid]){ for (const arr of pendingMacData[pid]) Factory.applyData(arr); delete pendingMacData[pid]; }
    endApply();
  }

  // ================= 本地操作钩子（广播到服务器） =================
  function ensurePatched(){
    if (patched) return;
    patched = true;
    const worldSet = World.set;
    World.set = function(x, y, z, id, silent){
      const cx = Math.floor(x / World.CHUNK), cz = Math.floor(z / World.CHUNK);
      const wasMod = World.chunkModified(cx, cz);
      worldSet(x, y, z, id, silent);
      if (active() && !isApplying() && gameReady() && !silent){
        const full = !wasMod ? World.serializeChunk(cx, cz) : undefined;
        broadcast({ t: 'blk', id: myId, planet: Game.currentPlanet, x, y, z, b: id, full });
      }
    };
    const facPlace = Factory.place;
    Factory.place = function(x, y, z, bk, dir){
      const r = facPlace(x, y, z, bk, dir);
      if (active() && !isApplying() && gameReady()){
        const m = Factory.at(x, y, z);
        broadcast({ t: 'mac', id: myId, planet: Game.currentPlanet, op: 'add', x, y, z, type: m ? m.type : (BLOCKS[bk] || {}).machine, dir, data: m ? m.data : undefined });
      }
      return r;
    };
    const facRemove = Factory.remove;
    Factory.remove = function(x, y, z){
      const r = facRemove(x, y, z);
      if (active() && !isApplying() && gameReady())
        broadcast({ t: 'mac', id: myId, planet: Game.currentPlanet, op: 'remove', x, y, z });
      return r;
    };
    // 生物击杀广播（血量通过快照对齐；死亡即时同步）
    const creKill = Creatures.kill;
    Creatures.kill = function(g, opts){
      const r = creKill(g, opts);
      if (active() && gameReady() && g && g.userData && Number.isFinite(g.userData.nid) && !(opts && opts.remote))
        broadcast({ t: 'cre-kill', id: myId, planet: Game.currentPlanet, cid: g.userData.nid });
      return r;
    };
  }

  // ================= 远程玩家化身 =================
  const STATION_STATES = ['station', 'docked', 'dockAnim', 'stationed', 'stationWalk', 'undockAnim'];
  function gameReady(){
    return window.Game && (Game.state === 'planet' || Game.state === 'space' || Game.state === 'atmo' || Game.state === 'atmoland' || Game.state === 'seated' || STATION_STATES.includes(Game.state));
  }
  function buildAvatar(id, name, app){
    const g = new THREE.Group();
    let fig;
    if (window.Humanoid){
      fig = Humanoid.build(Object.assign({}, app || {}, {
        trimOn: true, badge: true, jetpack: true,
        helmet: true, visor: (app && app.visor) || '#ffb347',
      }));
      g.add(fig);
    } else {
      const figG = new THREE.Group();
      g.add(figG);
      const suit = new THREE.MeshLambertMaterial({ color: 0x3fa8c9 });
      const dark = new THREE.MeshLambertMaterial({ color: 0x1d3a52 });
      const visor = new THREE.MeshLambertMaterial({ color: 0xffb347, emissive: 0x664411 });
      const B = (w, h, d, m, x, y, z) => { const mm = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), m); mm.position.set(x, y, z); figG.add(mm); return mm; };
      B(0.5, 0.62, 0.3, suit, 0, 0.62, 0);
      B(0.42, 0.4, 0.4, suit, 0, 1.18, 0);
      B(0.3, 0.14, 0.02, visor, 0, 1.2, -0.21);
      B(0.16, 0.5, 0.2, dark, -0.14, 0.15, 0);
      B(0.16, 0.5, 0.2, dark, 0.14, 0.15, 0);
      B(0.3, 0.4, 0.16, dark, 0, 0.72, 0.24);
      fig = figG;
    }
    // 名牌（显示玩家名字）
    const c = document.createElement('canvas'); c.width = 256; c.height = 40;
    const x = c.getContext('2d');
    x.font = 'bold 22px Consolas'; x.textAlign = 'center';
    x.fillStyle = '#35e0e8'; x.fillText((name || 'P' + id).slice(0, 16), 128, 27);
    const tag = new THREE.Sprite(new THREE.SpriteMaterial({ map: new THREE.CanvasTexture(c), transparent: true, depthWrite: false }));
    tag.scale.set(3.2, 0.5, 1);
    tag.position.y = 1.85;
    g.add(tag);
    // 采矿光束（动作位 1：队友在挖矿）
    const beamMat = new THREE.MeshBasicMaterial({ color: 0xff5533, transparent: true, opacity: 0.85, blending: THREE.AdditiveBlending, depthWrite: false });
    const beam = new THREE.Mesh(new THREE.CylinderGeometry(0.03, 0.03, 1, 5, 1, true), beamMat);
    beam.visible = false;
    beam.position.set(0, 1.15, -1.1);
    beam.rotation.x = Math.PI / 2;
    g.add(beam);
    // 小飞船（大气/太空态显示）
    const ship = new THREE.Group();
    const hull = new THREE.MeshLambertMaterial({ color: 0x9ab6c9 });
    const S = (w, h, d, x2, y2, z2) => { const mm = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), hull); mm.position.set(x2, y2, z2); ship.add(mm); return mm; };
    S(1.4, 0.9, 3.6, 0, 0, 0); S(2.6, 0.16, 1.4, -1.9, 0, 0.7); S(2.6, 0.16, 1.4, 1.9, 0, 0.7);
    ship.visible = false;
    g.add(ship);
    return { group: g, ship, beam, body: [fig], tag };
  }
  function rebuildAvatarFigure(r, app){
    if (!window.Humanoid) return;
    const fig = Humanoid.build(Object.assign({}, app || {}, {
      trimOn: true, badge: true, jetpack: true,
      helmet: true, visor: (app && app.visor) || '#ffb347',
    }));
    for (const o of r.body) r.group.remove(o);
    disposeObject3D(r.body[0], { skipGeo: true, skipTex: true, skipMat: true });
    r.group.add(fig);
    r.body = [fig];
    r.speed = 0;
  }
  function ensureRemote(id, name, app){
    let r = remotes.get(id);
    if (!r){
      const a = buildAvatar(id, name, app);
      r = { ...a, id, name: name || '', planet: -1, st: '', pos: new THREE.Vector3(), tgt: new THREE.Vector3(), yaw: 0, tyaw: 0, inScene: null, last: 0, app: app || null, speed: 0 };
      remotes.set(id, r);
      onStatus();
    } else {
      if (name && r.name !== name){
        r.name = name;
        const x = r.tag.material.map.image.getContext('2d');
        x.clearRect(0, 0, 256, 40);
        x.fillStyle = '#35e0e8'; x.fillText(name.slice(0, 16), 128, 27);
        r.tag.material.map.needsUpdate = true;
      }
      if (JSON.stringify(r.app || null) !== JSON.stringify(app || null)){
        r.app = app || null;
        rebuildAvatarFigure(r, app);
      }
    }
    return r;
  }
  function removeRemote(id){
    const r = remotes.get(id);
    if (r){
      if (r.inScene) r.inScene.remove(r.group);
      disposeObject3D(r.group);
    }
    remotes.delete(id);
  }
  function clearRemotes(){
    for (const id of [...remotes.keys()]) removeRemote(id);
  }
  function onPos(m){
    if (m.id === myId) return;
    if (!Array.isArray(m.p) || m.p.length < 3 || !m.p.every(Number.isFinite) || !Number.isFinite(m.yaw)) return;
    let app = null;
    if (m.app && typeof m.app === 'object' && !Array.isArray(m.app)){
      try { if (JSON.stringify(m.app).length < 800) app = m.app; } catch(e){ app = null; }
    }
    const nm = players.get(m.id);
    const r = ensureRemote(m.id, nm ? nm.name : '', app);
    r.planet = m.planet;
    r.st = m.st;
    r.act = m.act || 0;
    r.tgt.fromArray(m.p);
    r.tyaw = m.yaw;
    if (!r.last || performance.now() - r.last > 2000) r.pos.copy(r.tgt);
    r.last = performance.now();
  }

  // ================= 每帧 =================
  function myPosMsg(){
    const st = Game.state;
    let p, yaw;
    if (st === 'space'){ p = Space.shipState.pos.toArray(); yaw = Space.shipState.yaw; }
    else if (st === 'atmo' || st === 'atmoland' || st === 'seated' || st === 'launching'){ p = Game.shipPos.toArray(); yaw = Game.atmo.yaw; }
    else { p = Player.pos.toArray(); yaw = Player.yaw; }
    const app = Player.appearance || null;
    const appJson = JSON.stringify(app || null);
    const msg = { t: 'pos', id: myId, planet: Game.currentPlanet, st, p, yaw, act: Player.mineHeld ? 1 : 0 };
    if (appJson !== lastAppJson){
      lastAppJson = appJson;
      msg.app = app;
    }
    return msg;
  }
  function tick(dt){
    if (!connected){
      // 加载中挂起的 init：就绪后应用
      if (pendingInit && window.Game && Game.state !== 'loading') applyInit(pendingInit);
      return;
    }
    if (!window.Game) return;
    drainPending();
    if (pendingInit && Game.state !== 'loading') applyInit(pendingInit);
    pingT += dt;
    if (pingT > 20){ pingT = 0; broadcast({ t: 'ping' }); }

    if (gameReady()){
      posTimer += dt;
      if (posTimer > 0.1){ posTimer = 0; broadcast(myPosMsg()); }
      // 机器运行数据快照（2 秒，变更才发）
      macSyncT += dt;
      if (macSyncT > 2){
        macSyncT = 0;
        const arr = Factory.serialize().map(m => ({ x: m.x, y: m.y, z: m.z, data: m.data }));
        const j = JSON.stringify(arr);
        if (j !== lastMacData){
          lastMacData = j;
          if (arr.length) broadcast({ t: 'mac-data', id: myId, planet: Game.currentPlanet, arr });
        }
      }
      // 生物快照（1.2 秒）
      creSyncT += dt;
      if (creSyncT > 1.2){
        creSyncT = 0;
        const snap = Creatures.snapshot(Player.pos);
        if (snap) broadcast({ t: 'cre', id: myId, planet: Game.currentPlanet, arr: snap });
      }
      // 市场行情（3 秒，变更才发）
      marketSyncT += dt;
      if (marketSyncT > 3){
        marketSyncT = 0;
        const j = JSON.stringify(Game.market || {});
        if (j !== lastMarket){ lastMarket = j; broadcast({ t: 'market', id: myId, market: Game.market }); }
      }
      // 地图标记（5 秒，变更才发）
      markSyncT += dt;
      if (markSyncT > 5){
        markSyncT = 0;
        const j = JSON.stringify(Game.mapMarks || {});
        if (j !== lastMarks){ lastMarks = j; broadcast({ t: 'mapMarks', id: myId, mapMarks: Game.mapMarks }); }
      }
      // 人物数据（30 秒，变更才发；服务器按名字持久化）
      charSyncT += dt;
      if (charSyncT > 30){
        charSyncT = 0;
        if (Game.buildCharData){
          const j = JSON.stringify(Game.buildCharData());
          if (j !== lastChar){ lastChar = j; broadcast({ t: 'char', id: myId, char: Game.buildCharData() }); }
        }
      }
    }
    updateRemotes(dt);
  }

  function updateRemotes(dt){
    const now = performance.now();
    const expired = [];
    for (const [id, r] of remotes){
      const myState = Game.state;
      let scene = null, showShip = false;
      if ((myState === 'planet' || myState === 'seated' || myState === 'atmo' || myState === 'atmoland' || myState === 'launching') && r.planet === Game.currentPlanet){
        if (r.st === 'planet' || r.st === 'seated') scene = Game.planetScene;
        else if (r.st === 'atmo' || r.st === 'atmoland' || r.st === 'launching'){ scene = Game.planetScene; showShip = true; }
      } else if (myState === 'space' && r.st === 'space'){
        scene = Space.scene; showShip = true;
      }
      if (now - r.last > 8000){ expired.push(id); continue; }
      if (r.inScene !== scene){
        if (r.inScene) r.inScene.remove(r.group);
        if (scene) scene.add(r.group);
        r.inScene = scene;
      }
      if (!scene) continue;
      r.ship.visible = showShip;
      r.body.forEach(o => { if (o !== r.tag) o.visible = !showShip; });
      r.beam.visible = !showShip && (r.act & 1) === 1;
      const spd = Math.hypot(r.tgt.x - r.pos.x, r.tgt.z - r.pos.z);
      r.speed += (spd - r.speed) * Math.min(1, dt * 5);
      r.pos.lerp(r.tgt, Math.min(1, dt * 8));
      let dy = r.tyaw - r.yaw;
      dy = ((dy + Math.PI) % (Math.PI * 2) + Math.PI * 2) % (Math.PI * 2) - Math.PI;
      r.yaw += dy * Math.min(1, dt * 8);
      r.group.position.copy(r.pos);
      if (r.st === 'planet' || r.st === 'seated') r.group.position.y -= 1.62;
      r.group.rotation.y = r.yaw + Math.PI;
      r.group.visible = true;
      if (!showShip && window.Humanoid && r.body[0]){
        Humanoid.animate(r.body[0], dt, r.speed > 0.12, Math.min(7, r.speed));
      }
    }
    for (const id of expired){ removeRemote(id); onStatus(); }
  }
  function getRemotes(){
    const out = [];
    const now = performance.now();
    for (const [id, r] of remotes){
      if (now - r.last > 8000) continue;
      out.push({ id, name: r.name, planet: r.planet, st: r.st, pos: r.pos });
    }
    return out;
  }
  function getPlayers(){
    const out = [];
    for (const [id, p] of players) out.push({ id, name: p.name, planet: p.planet, st: p.st });
    return out;
  }

  // ================= 世界就绪（主机上传世界包） =================
  function onWorldReady(){
    if (role === 'host' && connected && serverInfo && !serverInfo.hasWorld && window.Game && Game.buildNetWorld){
      broadcast({ t: 'world-upload', world: Game.buildNetWorld() });
      serverInfo.hasWorld = true;
      waitingWorld = false;
      onStatus();
    }
  }
  function resetWorld(){
    if (role !== 'host' || !connected) return;
    broadcast({ t: 'reset-world' });
  }

  // ================= 时间同步 =================
  function timeSynced(){ return connected && gotInit; }
  function syncedTime(){
    return (dayT.v + (performance.now() - dayT.at) / 1000 / DAY_LEN) % 1;
  }

  // ================= 传送 =================
  function requestTp(id){
    if (!connected) return;
    broadcast({ t: 'tp', target: id });
  }

  // ================= 聊天 =================
  let chatBox = null, chatInput = null, chatOpen = false;
  function ensureChatUI(){
    if (chatBox) return;
    chatBox = document.createElement('div');
    chatBox.id = 'chatBox';
    const input = document.createElement('input');
    input.id = 'chatInput';
    input.maxLength = 200;
    input.placeholder = '输入消息，回车发送（/help 查看命令）';
    input.autocomplete = 'off';
    input.spellcheck = false;
    input.classList.add('hidden');   // 初始隐藏：此前收到第一条消息时输入框凭空出现在画面下方
    document.body.appendChild(chatBox);
    document.body.appendChild(input);
    chatInput = input;
    input.addEventListener('keydown', e => {
      if (e.key === 'Enter'){
        // 中文输入法候选确认（isComposing/keyCode 229）不得触发发送，否则拼音上屏即误发
        if (e.isComposing || e.keyCode === 229){ e.stopPropagation(); return; }
        e.preventDefault();
        const text = chatInput.value.trim();
        chatInput.value = '';
        if (text && connected) broadcast({ t: 'chat', text });
        closeChat();
      } else if (e.key === 'Escape'){
        e.preventDefault();
        closeChat();
      }
    });
    document.addEventListener('keydown', e => {
      if (e.target && (e.target.tagName === 'INPUT' || e.target.tagName === 'TEXTAREA' || e.target.tagName === 'BUTTON')) return;
      if (e.code === 'Enter'){
        if (e.isComposing || e.keyCode === 229) return;   // IME 候选确认不开聊天框
        if (!chatOpen && connected){
          const st = window.Game ? Game.state : 'menu';
          const panelOpen = window.UI && UI.anyPanelOpen && UI.anyPanelOpen();
          if (st === 'menu' || (!panelOpen && st !== 'loading')) openChat();
        }
        return;
      }
      if (e.code === 'KeyO' && !e.repeat && chatOpen === false){
        if (window.Game && Game.state !== 'menu' && Game.state !== 'loading') togglePlayers();
      }
    });
  }
  function openChat(){
    ensureChatUI();
    if (!connected) return;
    chatOpen = true;
    chatInput.classList.remove('hidden');
    chatInput.focus();
    try { if (document.pointerLockElement) document.exitPointerLock(); } catch(e){}
  }
  function closeChat(){
    if (!chatOpen) return;
    chatOpen = false;
    chatInput.classList.add('hidden');
    chatInput.blur();
    if (window.Game && Game.lockPointer && Game.state !== 'menu') setTimeout(() => Game.lockPointer(), 60);
  }
  function isChatOpen(){ return chatOpen; }
  function addChatLine(name, text, sys){
    ensureChatUI();
    const line = document.createElement('div');
    line.className = 'chat-line' + (sys ? ' sys' : '');
    if (sys){
      line.textContent = text;
    } else {
      const nm = document.createElement('span');
      nm.className = 'chat-name';
      nm.textContent = name + '：';
      line.appendChild(nm);
      line.appendChild(document.createTextNode(text));
    }
    chatBox.appendChild(line);
    while (chatBox.children.length > 24) chatBox.firstChild.remove();
    chatBox.classList.add('show');
    clearTimeout(chatBox._hideT);
    chatBox._hideT = setTimeout(() => chatBox.classList.remove('show'), 12000);
    try { if (window.Sound) Sound.play('msg'); } catch(e){}
  }
  function onChat(m){
    if (m.sys){
      addChatLine('', m.text, true);
      return;
    }
    if (!m.name) return;
    addChatLine(m.name, String(m.text || ''));
  }

  // ================= 玩家列表面板 =================
  let playersPanel = null, playersListEl = null;
  function ensurePlayersUI(){
    if (playersPanel) return;
    playersPanel = document.createElement('div');
    playersPanel.id = 'playersPanel';
    playersPanel.className = 'hidden';
    const head = document.createElement('div');
    head.className = 'panel-head';
    head.innerHTML = '<span>◈ 在线玩家</span><button class="pclose">✕</button>';
    head.querySelector('.pclose').onclick = () => playersPanel.classList.add('hidden');
    playersPanel.appendChild(head);
    playersListEl = document.createElement('div');
    playersListEl.className = 'players-list';
    playersPanel.appendChild(playersListEl);
    document.body.appendChild(playersPanel);
  }
  const STATE_NAMES = { planet: '星球地表', space: '太空', atmo: '大气层', atmoland: '降落中', seated: '座舱', station: '空间站', docked: '空间站', menu: '主菜单', loading: '加载中' };
  function refreshPlayersUI(){
    if (!playersListEl) return;
    playersListEl.innerHTML = '';
    const rows = [];
    for (const p of players.values()) rows.push(p);
    for (const r of remotes.values()){
      const p = players.get(r.id);
      if (p){ p.planet = r.planet; p.st = r.st; }
    }
    if (!rows.length){
      const d = document.createElement('div');
      d.className = 'save-empty';
      d.textContent = '— 暂无其他玩家 —';
      playersListEl.appendChild(d);
      return;
    }
    for (const p of rows){
      const row = document.createElement('div');
      row.className = 'player-row';
      const nm = document.createElement('span');
      nm.className = 'player-name';
      nm.textContent = p.id === myId ? p.name + '（我）' : p.name;
      const st = document.createElement('span');
      st.className = 'player-st';
      const planetName = (window.SYSTEM_PLANETS && Number.isInteger(p.planet) && SYSTEM_PLANETS[p.planet]) ? SYSTEM_PLANETS[p.planet].name + ' · ' : '';
      st.textContent = planetName + (STATE_NAMES[p.st] || (p.st || '未知'));
      row.appendChild(nm);
      row.appendChild(st);
      if (p.id !== myId){
        const btn = document.createElement('button');
        btn.className = 'boot-btn small tp-btn';
        btn.textContent = '⇄ 传送';
        btn.onclick = () => { requestTp(p.id); Sound.play('uiClick'); };
        row.appendChild(btn);
      }
      playersListEl.appendChild(row);
    }
  }
  function togglePlayers(){
    ensurePlayersUI();
    if (!connected) return;
    refreshPlayersUI();
    playersPanel.classList.toggle('hidden');
    if (!playersPanel.classList.contains('hidden')) Sound.play('uiOpen');
  }

  // ================= 出口 =================
  return {
    hostRoom, joinRoom, disconnect, tick, onWorldReady, resetWorld, getRemotes, getPlayers,
    requestTp, defaultAddr, sendChat(text){ if (connected) broadcast({ t: 'chat', text }); },
    openChat, closeChat, isChatOpen, togglePlayers, addChatLine,
    ensureChatUI, ensurePlayersUI,
    timeSynced, syncedTime,
    get role(){ return role; },
    get myId(){ return myId; },
    get myName(){ return myName; },
    get serverInfo(){ return serverInfo; },
    get waitingWorld(){ return waitingWorld; },
    get gotInit(){ return gotInit; },
    active, status,
    set statusChanged(fn){ onStatus = fn || (() => {}); },
    set toastChanged(fn){ onToast = fn || onToast; },
  };
})();
window.Net = Net;

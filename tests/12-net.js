/* STARFORGE 测试套件 12 — 跨平台联机（server.mjs 协议 + 客户端 Net 模块）
   前置：test/run.mjs 会拉起一个隔离存档的联机服务器（http :17887 / ws :17886） */
__SF_TEST__.suite('net', function (t, api) {
  var A = api.assert;
  var HTTP = 'http://127.0.0.1:17887';
  var WS = 'ws://127.0.0.1:17886';
  var sleep = api.sleep;
  var HOSTKEY = '';   // 主机所有权密钥（服务器在首次世界上传时签发，后续声明主机必须出示）

  function wsClient() {
    var ws = new WebSocket(WS);
    var waiters = [];
    ws.onmessage = function (e) {
      var m;
      try { m = JSON.parse(e.data); } catch (err) { return; }
      waiters.splice(0).forEach(function (w) { w(m); });
    };
    return {
      ws: ws,
      open: new Promise(function (res, rej) { ws.onopen = res; ws.onerror = function () { rej(new Error('ws 连接失败（服务器未启动？）')); }; }),
      send: function (o) { ws.send(JSON.stringify(o)); },
      next: function (pred, ms) {
        return new Promise(function (res, rej) {
          var to = setTimeout(function () { rej(new Error('等待消息超时: ' + pred)); }, ms || 6000);
          var check = function (m) {
            if (!pred || pred(m)) { clearTimeout(to); res(m); }
            else waiters.push(check);
          };
          waiters.push(check);
        });
      },
      close: function () { try { ws.close(); } catch (e) {} },
    };
  }

  // 服务器世界置空（幂等）
  t.before(async function () {
    var c = wsClient();
    await c.open;
    c.send({ t: 'hello', v: 4, name: '测试清理者', role: 'host', hostKey: HOSTKEY });
    var id = await c.next(function (m) { return m.t === 'ws-id'; });
    if (id.hasWorld) {
      c.send({ t: 'reset-world' });
      await c.next(function (m) { return m.t === 'world-missing'; });
    }
    c.close();
    await sleep(300);
  });

  t.test('客户端 Net 模块连接（host:port 自定义端口）', async function () {
    await Net.joinRoom('127.0.0.1:17886');
    A.eq(Net.active(), true, 'Net 已连接');
    A.ok(Net.myId >= 1, '获得服务器分配 id');
    A.ok(Net.serverInfo && typeof Net.serverInfo.svName === 'string' && Net.serverInfo.svName.length > 0, '服务器信息');
    await sleep(300);   // world-missing 紧随 ws-id 到达
    A.eq(Net.waitingWorld, true, '空世界 → 等待世界标志');
    A.eq(Net.role, 'guest', '角色为成员');
    Net.disconnect();
    A.eq(Net.active(), false, '断开连接');
  });

  t.test('HTTP 静态服务与状态端点', async function () {
    var r;
    try { r = await fetch(HTTP + '/index.html'); }
    catch (e) { throw new Error('fetch#1 ' + e.message + ' | ' + e.name + ' | origin=' + location.origin + ' | url=' + HTTP); }
    A.eq(r.status, 200, 'index.html 200');
    var txt = await r.text();
    A.ok(txt.indexOf('STARFORGE') >= 0, '页面包含游戏标题');
    var st;
    try { st = await fetch(HTTP + '/__status').then(function (x) { return x.json(); }); }
    catch (e) { throw new Error('fetch#2 ' + e.message + ' | ' + e.name); }
    A.eq(st.ok, true, '状态端点 ok');
    A.eq(st.wsPort, 17886, '报告 ws 端口');
    A.eq(st.hasWorld, false, '当前无世界');
    var r2;
    try { r2 = await fetch(HTTP + '/不存在.html'); }
    catch (e) { throw new Error('fetch#3 ' + e.message + ' | ' + e.name); }
    A.eq(r2.status, 404, '未知文件 404（跨域可读）');
    // 敏感文件不得经静态服务器暴露：配置文件含服务器密码，世界存档含主机密钥与玩家数据
    var r3 = await fetch(HTTP + '/server-config.json');
    A.eq(r3.status, 404, 'server-config.json 不可读（' + r3.status + '）');
    var r4 = await fetch(HTTP + '/package.json');
    A.eq(r4.status, 404, 'package.json 不可读（' + r4.status + '）');
    var r5 = await fetch(HTTP + '/server.mjs');
    A.eq(r5.status, 404, 'server.mjs 源码不可读（' + r5.status + '）');
    var r6 = await fetch(HTTP + '/.git/config');
    A.eq(r6.status, 403, '.git 不可读（' + r6.status + '）');
    // 路径穿越防护由 test/run.mjs 用原始 TCP 请求自检（浏览器会规范化 URL，无法在页面内测）
  });

  t.test('握手 → 空世界 → 上传 → 访客自动收 init', async function () {
    var host = wsClient(); await host.open;
    host.send({ t: 'hello', v: 4, name: '测试房主', role: 'host', app: { suit: '#123456' } });
    var id = await host.next(function (m) { return m.t === 'ws-id'; });
    A.eq(id.auth, 'ok', '认证通过');
    A.eq(id.role, 'host', '空世界首个声明者为真实主机');
    await host.next(function (m) { return m.t === 'world-missing'; });
    host.send({ t: 'world-upload', world: {
      v: 4, name: '联机测试世界', creative: false, dropMult: 4,
      galaxySeed: 777, galaxyCount: 1, currentPlanet: 0, dayTime: 0.4,
      planets: { '0': { mods: {}, machines: [], shipPos: [5, 30, 5], seed: 777, biome: 'green' } },
      galaxyArchives: {}, market: { carbon: 1.2 }, mapMarks: { '0': [{ x: 3, z: 4, y: 30, label: '矿点', gal: false }] }, flags: { x: 1 }, warpLock: null,
    } });
    // 首次上传签发主机所有权密钥
    var hk = await host.next(function (m) { return m.t === 'host-key'; });
    A.ok(typeof hk.key === 'string' && hk.key.length >= 16, '签发主机密钥');
    HOSTKEY = hk.key;
    await sleep(300);
    var guest = wsClient(); await guest.open;
    guest.send({ t: 'hello', v: 4, name: '测试访客', role: 'guest' });
    var init = await guest.next(function (m) { return m.t === 'init'; });
    A.eq(init.world.name, '联机测试世界', '世界名');
    A.eq(init.world.planets['0'].seed, 777, '星球种子');
    A.eq(init.world.market.carbon, 1.2, '市场行情同步');
    A.ok(init.world.dayTime >= 0.39 && init.world.dayTime < 0.42, '昼夜时间同步');
    A.eq(init.world.mapMarks['0'][0].label, '矿点', '地图标记同步');
    A.eq(init.you.name, '测试访客', '服务器确认名字');
    A.eq(init.spawn, null, '首次加入无重生点');
    host.close(); guest.close();
  });

  t.test('方块改动：整块 RLE + 增量 + 持久化到新访客', async function () {
    // 先连入拿到现有世界（世界已有密钥，必须出示才能声明主机）
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '方块甲', role: 'host', hostKey: HOSTKEY });
    await a.next(function (m) { return m.t === 'init' || m.t === 'world-missing'; });
    // 整块（未知区块必须带 full RLE；区块数据长度 = 16×16×96）
    var full = [];
    var i;
    for (i = 0; i < 24576; i++) full.push(0);
    full[((3 * 16 + 4) * 16) + 5] = 1;   // x=5,y=3,z=4
    var rle = [], cur = full[0], run = 1;
    for (i = 1; i < full.length; i++) {
      if (full[i] === cur && run < 65535) run++;
      else { rle.push(run, cur); cur = full[i]; run = 1; }
    }
    rle.push(run, cur);
    var b = wsClient(); await b.open;
    b.send({ t: 'hello', v: 4, name: '方块乙', role: 'guest' });
    await b.next(function (m) { return m.t === 'init'; });
    b.send({ t: 'blk', planet: 0, x: 5, y: 3, z: 4, b: 1, full: rle });
    await a.next(function (m) { return m.t === 'blk' && m.b === 1; });
    b.send({ t: 'blk', planet: 0, x: 6, y: 3, z: 4, b: 2 });   // 同区块增量，不带 full
    await a.next(function (m) { return m.t === 'blk' && m.b === 2; });
    // 持久化验证：第三个连接拿到两份改动
    var c = wsClient(); await c.open;
    c.send({ t: 'hello', v: 4, name: '方块丙', role: 'guest' });
    var init = await c.next(function (m) { return m.t === 'init'; });
    A.ok(init.world.planets['0'].mods['0,0'], '修改区块已持久化');
    a.close(); b.close(); c.close();
  });

  t.test('未认证连接（不发 hello）不得修改世界或广播', async function () {
    var a = wsClient(); await a.open;
    // 被动收集旁观者收到的所有消息（保留原 onmessage 派发）
    var seen = [];
    var orig = a.ws.onmessage;
    a.ws.onmessage = function (e) {
      if (orig) orig.call(a.ws, e);
      try { seen.push(JSON.parse(e.data)); } catch (err) {}
    };
    a.send({ t: 'hello', v: 4, name: '旁观者', role: 'guest' });
    await a.next(function (m) { return m.t === 'init'; });
    // 攻击者：完成 WebSocket 握手后直接发状态修改消息，完全跳过 hello（无密码/无名字）
    var atk = wsClient(); await atk.open;
    var full = [24576, 0];   // 整块空气 RLE（未知区块必须携带 full）
    atk.send({ t: 'blk', planet: 0, x: 20, y: 3, z: 20, b: 1, full: full });   // chunk '1,1'
    atk.send({ t: 'chat', text: '未认证注入' });
    await sleep(600);
    A.eq(seen.some(function (m) { return m.t === 'blk' && m.x === 20; }), false, '旁观者未收到未认证 blk 广播');
    A.eq(seen.some(function (m) { return m.t === 'chat' && m.text === '未认证注入'; }), false, '旁观者未收到未认证聊天');
    // 新连接读取服务器世界：攻击者的方块改动不得落盘
    var c = wsClient(); await c.open;
    c.send({ t: 'hello', v: 4, name: '验证者', role: 'guest' });
    var init = await c.next(function (m) { return m.t === 'init'; });
    A.eq(init.world.planets['0'].mods['1,1'], undefined, '未认证 blk 未写入世界（chunk 1,1 不存在）');
    a.close(); atk.close(); c.close();
  });

  t.test('机器增删 + 运行数据合并持久化', async function () {
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '机器甲', role: 'guest' });
    await a.next(function (m) { return m.t === 'init'; });
    a.send({ t: 'mac', planet: 0, op: 'add', x: 8, y: 2, z: 8, type: 'furnace', dir: 0, data: { fuel: null } });
    a.send({ t: 'mac-data', planet: 0, arr: [{ x: 8, y: 2, z: 8, data: { fuel: { item: 'carbon', n: 3 }, prog: 0.5 } }] });
    await sleep(300);
    var b = wsClient(); await b.open;
    b.send({ t: 'hello', v: 4, name: '机器乙', role: 'guest' });
    var init = await b.next(function (m) { return m.t === 'init'; });
    var mach = (init.world.planets['0'].machines || []).filter(function (m) { return m.x === 8; })[0];
    A.ok(mach && mach.type === 'furnace', '机器已持久化');
    A.eq(mach.data.fuel.n, 3, '燃料数据已持久化');
    A.ok(Math.abs(mach.data.prog - 0.5) < 1e-9, '进度数据已持久化');
    // 删除
    a.send({ t: 'mac', planet: 0, op: 'remove', x: 8, y: 2, z: 8 });
    await sleep(300);
    var c = wsClient(); await c.open;
    c.send({ t: 'hello', v: 4, name: '机器丙', role: 'guest' });
    var init2 = await c.next(function (m) { return m.t === 'init'; });
    A.eq((init2.world.planets['0'].machines || []).some(function (m) { return m.x === 8; }), false, '机器已删除');
    a.close(); b.close(); c.close();
  });

  t.test('聊天中继 + 服务器命令', async function () {
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '聊天甲', role: 'guest' });
    await a.next(function (m) { return m.t === 'init'; });
    var b = wsClient(); await b.open;
    b.send({ t: 'hello', v: 4, name: '聊天乙', role: 'guest' });
    await b.next(function (m) { return m.t === 'init'; });
    b.send({ t: 'chat', text: '大家好' });
    var chat = await a.next(function (m) { return m.t === 'chat' && !m.sys; });
    A.eq(chat.name, '聊天乙', '聊天来源名');
    A.eq(chat.text, '大家好', '聊天内容');
    a.send({ t: 'chat', text: '/list' });
    var lst = await a.next(function (m) { return m.t === 'chat' && m.sys && m.text.indexOf('聊天乙') >= 0; });
    A.ok(lst, '/list 列出在线玩家');
    a.close(); b.close();
  });

  t.test('位置中继 + 一键传送 + 市场/标记/生物中继', async function () {
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '传送甲', role: 'guest' });
    await a.next(function (m) { return m.t === 'init'; });
    var b = wsClient(); await b.open;
    b.send({ t: 'hello', v: 4, name: '传送乙', role: 'guest' });
    var bInit = await b.next(function (m) { return m.t === 'init'; });
    var bId = bInit.you.id;
    b.send({ t: 'pos', planet: 0, st: 'planet', p: [11, 22, 33], yaw: 1.5, act: 1 });
    var pos = await a.next(function (m) { return m.t === 'pos' && m.id === bId; });
    A.ok(pos.p[0] === 11 && pos.p[2] === 33, '位置中继');
    A.eq(pos.act, 1, '动作位中继');
    a.send({ t: 'tp', target: bId });
    var tp = await a.next(function (m) { return m.t === 'tp-you'; });
    A.eq(tp.planet, 0, '传送星球');
    A.eq(tp.p[2], 33, '传送坐标');
    A.eq(tp.target, '传送乙', '传送目标名');
    // 市场 / 标记 / 生物快照中继
    b.send({ t: 'market', market: { carbon: 1.7 } });
    var mk = await a.next(function (m) { return m.t === 'market'; });
    A.eq(mk.market.carbon, 1.7, '市场中继');
    b.send({ t: 'mapMarks', mapMarks: { '0': [{ x: 9, z: 9, y: 30, label: '队友标记', gal: true }] } });
    var mm = await a.next(function (m) { return m.t === 'mapMarks'; });
    A.eq(mm.mapMarks['0'][0].label, '队友标记', '标记中继');
    b.send({ t: 'cre', planet: 0, arr: [[12345, 100, 200, 300, 90, 1, 4, 0]] });
    var cr = await a.next(function (m) { return m.t === 'cre'; });
    A.eq(cr.arr[0][0], 12345, '生物快照中继');
    a.close(); b.close();
  });

  t.test('pos 中继清洗：超长坐标数组/超大外观/异常动作位不被放大转发', async function () {
    var a = wsClient(); await a.open;
    // 旁观者被动收集所有 pos 消息
    var seen = [];
    var orig = a.ws.onmessage;
    a.ws.onmessage = function (e) {
      if (orig) orig.call(a.ws, e);
      try { var m = JSON.parse(e.data); if (m.t === 'pos') seen.push(m); } catch (err) {}
    };
    a.send({ t: 'hello', v: 4, name: '旁观甲', role: 'guest' });
    await a.next(function (m) { return m.t === 'init'; });
    var b = wsClient(); await b.open;
    b.send({ t: 'hello', v: 4, name: '中继乙', role: 'guest' });
    var bInit = await b.next(function (m) { return m.t === 'init'; });
    var bId = bInit.you.id;
    // 1) 超长坐标数组：整体拒绝（不得中继，也不得覆盖服务器记录的最后位置）
    var big = [1, 2, 3];
    var i;
    for (i = 0; i < 2000; i++) big.push(i);
    b.send({ t: 'pos', planet: 0, st: 'planet', p: big, yaw: 0 });
    // 2) 合法坐标 + 超大外观：位置正常中继，app 被剥离
    var bigApp = { suit: '#111111', junk: '' };
    for (i = 0; i < 5000; i++) bigApp.junk += 'x';
    b.send({ t: 'pos', planet: 0, st: 'planet', p: [21, 22, 23], yaw: 0.5, app: bigApp });
    // 3) 异常动作位（对象）：act 被剥离
    b.send({ t: 'pos', planet: 0, st: 'planet', p: [31, 32, 33], yaw: 0.5, act: { evil: 1 } });
    // 轮询等待中继消息到位（固定短 sleep 在高负载下偶发漏收 → 断言抖动）
    var normal = [];
    await api.waitUntil(function () {
      normal = seen.filter(function (m) { return m.id === bId; });
      return normal.some(function (m) { return m.p[0] === 21; }) && normal.some(function (m) { return m.p[0] === 31; });
    }, 5000, 50);
    A.eq(normal.some(function (m) { return m.p.length > 3; }), false, '超长坐标数组未被中继');
    A.eq(normal.some(function (m) { return m.p[0] === 1; }), false, '超长坐标消息整体被丢弃');
    var okApp = normal.filter(function (m) { return m.p[0] === 21; })[0];
    A.ok(okApp, '合法坐标正常中继');
    A.eq(okApp.app, undefined, '超大外观被剥离');
    var okAct = normal.filter(function (m) { return m.p[0] === 31; })[0];
    A.ok(okAct, '合法坐标正常中继（act 测试）');
    A.eq(okAct.act, undefined, '异常动作位被剥离');
    a.close(); b.close();
  });

  t.test('服务器权威昼夜时间广播', async function () {
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '时间甲', role: 'guest' });
    await a.next(function (m) { return m.t === 'init'; });
    var t1 = await a.next(function (m) { return m.t === 'time'; });
    var t2 = await a.next(function (m) { return m.t === 'time'; });
    A.ok(Number.isFinite(t1.dayTime) && Number.isFinite(t2.dayTime), '时间值有效');
    A.ok(t2.dayTime !== t1.dayTime, '时间在推进');
    a.close();
  });

  t.test('人物数据按名字持久化 + 同名重连恢复 + 离线上报', async function () {
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '人物甲', role: 'guest' });
    var idA = await a.next(function (m) { return m.t === 'ws-id' && m.auth === 'ok'; });
    A.ok(idA.token && idA.token.length >= 16, '首次使用名字签发身份令牌');
    await a.next(function (m) { return m.t === 'init'; });
    a.send({ t: 'pos', planet: 0, st: 'planet', p: [42, 30, 42], yaw: 0.5 });
    a.send({ t: 'char', char: { name: '人物甲', inv: [{ item: 'carbon', n: 9 }], credits: 100, player: { pos: [42, 30, 42], inv: [{ item: 'carbon', n: 9 }], credits: 100 }, techState: { survival: true } } });
    // 等第三个旁观者确认离线后再重连（避免重名改名）
    var watch = wsClient(); await watch.open;
    watch.send({ t: 'hello', v: 4, name: '见证者', role: 'guest' });
    await watch.next(function (m) { return m.t === 'init'; });
    a.close();
    await watch.next(function (m) { return m.t === 'left' && m.name === '人物甲'; }, 8000);
    var b = wsClient(); await b.open;
    b.send({ t: 'hello', v: 4, name: '人物甲', role: 'guest', token: idA.token });
    var init = await b.next(function (m) { return m.t === 'init'; });
    A.eq(init.you.name, '人物甲', '同名重连不改名');
    A.eq(init.you.char.credits, 100, '人物数据恢复');
    A.eq(init.you.char.inv[0].n, 9, '背包恢复');
    A.ok(init.spawn && init.spawn.p[0] === 42, '重生点恢复');
    watch.close(); b.close();
  });

  t.test('同名冒领防护：无令牌/错令牌被拒，持令牌可载入档案', async function () {
    // 先造档案：名字 X + 令牌 T
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '守擂者', role: 'guest' });
    var idA = await a.next(function (m) { return m.t === 'ws-id' && m.auth === 'ok'; });
    await a.next(function (m) { return m.t === 'init'; });
    a.send({ t: 'char', char: { name: '守擂者', credits: 777, player: { credits: 777, inv: [], pos: [1, 40, 1] }, techState: {} } });
    // 等旁观者确认离线，避免重名自动加 #2
    var watch = wsClient(); await watch.open;
    watch.send({ t: 'hello', v: 4, name: '旁观看客', role: 'guest' });
    await watch.next(function (m) { return m.t === 'init'; });
    a.close();
    await watch.next(function (m) { return m.t === 'left' && m.name === '守擂者'; }, 8000);
    // 冒领者：不带令牌 → 拒绝
    var atk = wsClient(); await atk.open;
    atk.send({ t: 'hello', v: 4, name: '守擂者', role: 'guest' });
    var err1 = await atk.next(function (m) { return m.t === 'ws-err'; });
    A.eq(err1.reason, 'name-taken', '无令牌冒领被拒');
    atk.close();
    // 冒领者：错误令牌 → 拒绝
    var atk2 = wsClient(); await atk2.open;
    atk2.send({ t: 'hello', v: 4, name: '守擂者', role: 'guest', token: 'x'.repeat(32) });
    var err2 = await atk2.next(function (m) { return m.t === 'ws-err'; });
    A.eq(err2.reason, 'name-taken', '错令牌冒领被拒');
    atk2.close();
    // 本人：持令牌 → 档案载入
    var me = wsClient(); await me.open;
    me.send({ t: 'hello', v: 4, name: '守擂者', role: 'guest', token: idA.token });
    var init = await me.next(function (m) { return m.t === 'init'; });
    A.eq(init.you.char.credits, 777, '本人持令牌载入档案');
    watch.close(); me.close();
  });

  t.test('密码校验与满员拒绝', async function () {
    // 无密码服务器对错误密码不敏感（服务器未配置密码）；验证错误版本被拒
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 1, name: '旧版本', role: 'guest' });
    var err = await a.next(function (m) { return m.t === 'ws-err'; });
    A.eq(err.reason, 'version', '版本不匹配被拒');
    a.close();
  });

  t.test('主机所有权：无密钥声明被降级，持密钥可重新声明', async function () {
    A.ok(HOSTKEY, '前置测试已签发密钥');
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '冒牌主机', role: 'host' });   // 不带密钥
    var idA = await a.next(function (m) { return m.t === 'ws-id'; });
    A.eq(idA.role, 'guest', '世界已有归属时无密钥声明主机被降级为成员');
    a.close();
    var b = wsClient(); await b.open;
    b.send({ t: 'hello', v: 4, name: '回归主机', role: 'host', hostKey: HOSTKEY });
    var idB = await b.next(function (m) { return m.t === 'ws-id'; });
    A.eq(idB.role, 'host', '持密钥可在主机离线后重新声明');
    b.close();
  });

  t.test('主机在线时他人抢座被降级', async function () {
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '在线主机', role: 'host', hostKey: HOSTKEY });
    var idA = await a.next(function (m) { return m.t === 'ws-id'; });
    A.eq(idA.role, 'host', '主机上线');
    var b = wsClient(); await b.open;
    b.send({ t: 'hello', v: 4, name: '抢座者', role: 'host', hostKey: HOSTKEY });
    var idB = await b.next(function (m) { return m.t === 'ws-id'; });
    A.eq(idB.role, 'guest', '主机在线时即使持密钥也被降级');
    a.close(); b.close();
  });

  t.test('晚加入玩家的 ws-id 名单包含先到玩家', async function () {
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '先到者', role: 'guest' });
    await a.next(function (m) { return m.t === 'init'; });
    var b = wsClient(); await b.open;
    b.send({ t: 'hello', v: 4, name: '后到者', role: 'guest' });
    var idB = await b.next(function (m) { return m.t === 'ws-id'; });
    A.ok((idB.players || []).some(function (p) { return p.name === '先到者'; }), '晚加入者能看到先到玩家');
    A.ok((idB.players || []).some(function (p) { return p.name === '后到者'; }), '名单包含自己');
    a.close(); b.close();
  });

  t.test('market 键数上限 + 上传包自由字段大小上限', async function () {
    // 1) market：5000 键只接受前 128 个合法键
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '行情甲', role: 'guest' });
    var initA = await a.next(function (m) { return m.t === 'init'; });
    var before = Object.keys(initA.world.market || {}).length;
    var b = wsClient(); await b.open;
    b.send({ t: 'hello', v: 4, name: '行情乙', role: 'guest' });
    await b.next(function (m) { return m.t === 'init'; });
    var big = {};
    var i;
    for (i = 0; i < 5000; i++) big['k' + i] = 1.5;
    b.send({ t: 'market', market: big });
    var mk = await a.next(function (m) { return m.t === 'market'; });
    A.ok(Object.keys(mk.market).length <= before + 128, 'market keys capped at 128 per message (before=' + before + ', got ' + Object.keys(mk.market).length + ')');
    a.close(); b.close();
    // 2) 上传包：超大 flags / galaxyArchives / warpLock 被丢弃（不入内存/存档/init 广播）
    var h = wsClient(); await h.open;
    h.send({ t: 'hello', v: 4, name: '上限主机', role: 'host', hostKey: HOSTKEY });
    await h.next(function (m) { return m.t === 'init' || m.t === 'world-missing'; });
    var giant = '';
    for (i = 0; i < 200000; i++) giant += 'x';
    var huge = '';
    for (i = 0; i < 600000; i++) huge += 'x';   // galaxyArchives 上限 512KB：600KB 应被丢弃
    h.send({ t: 'world-upload', world: {
      v: 4, name: '上限世界', creative: false, dropMult: 4,
      galaxySeed: 777, galaxyCount: 1, currentPlanet: 0, dayTime: 0.4,
      planets: { '0': { mods: {}, machines: [], shipPos: [5, 30, 5], seed: 777, biome: 'green' } },
      galaxyArchives: { big: huge }, market: {}, mapMarks: {}, flags: { big: giant }, warpLock: { big: giant },
    }});
    await sleep(300);
    var g = wsClient(); await g.open;
    g.send({ t: 'hello', v: 4, name: '上限访客', role: 'guest' });
    var init = await g.next(function (m) { return m.t === 'init'; });
    A.eq(Object.keys(init.world.flags).length, 0, 'giant flags dropped');
    A.eq(Object.keys(init.world.galaxyArchives).length, 0, 'giant galaxyArchives dropped');
    A.eq(init.world.warpLock, null, 'giant warpLock dropped');
    h.close(); g.close();
  });

  t.test('init 在 Game 未挂载时到达：gotInit 仍置位（昼夜同步不失效）', async function () {
    // 页面极早期（Game 未就绪）收到 init：此前早退漏设 gotInit → timeSynced() 恒假
    var G = window.Game;
    try {
      window.Game = undefined;
      await Net.joinRoom('127.0.0.1:17886');
      await api.waitUntil(function () { return Net.gotInit === true; }, 6000, 50);
      A.eq(Net.gotInit, true, 'gotInit set even when Game undefined');
      A.eq(Net.timeSynced(), true, 'timeSynced true after init without Game');
    } finally {
      Net.disconnect();   // 清空 pendingInit，避免 Game 恢复后被重新应用
      window.Game = G;
    }
  });

  t.test('断线/重连清理挂起增量：残留队列不在新世界上重放', async function () {
    await Net.joinRoom('127.0.0.1:17886');
    A.eq(Net.active(), true, 'Net connected');
    // 原始连接向「非当前星球」放方块 → 本页 onBlk 进挂起队列
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '增量注入者', role: 'guest' });
    await a.next(function (m) { return m.t === 'init'; });
    a.send({ t: 'blk', planet: 1, x: 3, y: 40, z: 3, b: 1, full: [24576, 0] });
    await api.waitUntil(function () { return Net.debugPendingCounts().blk === 1; }, 5000, 50);
    A.eq(Net.debugPendingCounts().blk, 1, 'blk for other planet queued');
    Net.disconnect();
    A.eq(Net.debugPendingCounts().blk, 0, 'pending queue cleared on disconnect');
    a.close();
  });

  t.test('重名去重：确认名（含 #N）持久化，重连不误领他人档案', async function () {
    var a = wsClient(); await a.open;
    a.send({ t: 'hello', v: 4, name: '占名者', role: 'guest' });
    await a.next(function (m) { return m.t === 'init'; });
    var inp = document.getElementById('netName');
    if (inp) inp.value = '占名者';
    await Net.joinRoom('127.0.0.1:17886');
    A.eq(Net.myName, '占名者#2', 'server-confirmed deduped name');
    A.eq(localStorage.getItem('starforge_net_name'), '占名者#2', 'full confirmed name persisted (with #2 suffix)');
    Net.disconnect();
    try { localStorage.removeItem('starforge_net_name'); } catch (e) {}
    if (inp) inp.value = '';   // 清空输入框：否则后续套件的 joinRoom 会沿用「占名者」误撞档案
    a.close();
  });
});

/* STARFORGE 测试套件 12b — 客户端世界包应用（Game.joinGame 全量重建） */
__SF_TEST__.suite('net-join', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('应用服务器世界包：全量世界 + 角色保留 + 出生点', async function () {
    // 先给本机角色设置可辨识状态（游戏内加入 → 保留本机角色）
    api.clearInv();
    api.setCredits(666);
    api.give('carbon', 30);
    // 与当前世界不同的种子，验证世界确实被替换
    var world = {
      v: 4, name: '服务器世界', creative: true, dropMult: 7,
      galaxySeed: 123456, galaxyCount: 1, currentPlanet: 0, dayTime: 0.25,
      planets: { '0': { mods: {}, machines: [], shipPos: [6, 40, 6], seed: 20202020, biome: 'lush' } },
      galaxyArchives: {}, market: { carbon: 1.5 }, mapMarks: { '0': [{ x: 10, z: 10, y: 30, label: '基地', gal: false }] },
      flags: { netFlag: true }, warpLock: null,
    };
    await Game.joinGame(Object.assign({}, world, { spawn: null, you: null }));
    A.eq(Game.state, 'planet', '已进入星球态');
    A.eq(Game.creative, true, '创造模式来自服务器');
    A.eq(Game.dropMult, 7, '难度倍率来自服务器');
    A.eq(World.seed, 20202020, '星球种子来自服务器');
    A.eq(World.biome.name, api.defs.BIOMES[api.defs.SYSTEM_PLANETS[0].biome].name, '生态随服务器世界');
    A.eq(Game.market.carbon, 1.5, '市场行情应用');
    A.eq(Game.mapMarks['0'][0].label, '基地', '地图标记应用');
    A.eq(Game.flags.netFlag, true, '世界旗标应用');
    A.eq(api.galaxySeed(), 123456, '星系种子应用');
    A.ok(Math.abs(Game.dayTime - 0.25) < 0.02, '昼夜时间应用');
    A.eq(api.credits(), 666, '游戏内加入保留本机角色（信用点）');
    A.eq(api.count('carbon'), 30, '游戏内加入保留本机角色（背包）');
    var p = api.pos();
    A.ok(Number.isFinite(p[1]) && p[1] > 0, '出生点在地表');
  });

  // 回归：joinGame 应用服务器世界期间，并发的新游戏建档流程跑
  // buildWorldData→savePlanetState 把 visitedPlanets 覆盖成旧世界——genPlanet 的
  // 50ms 等待窗口内读到被覆盖的种子，服务器世界被本地旧种子重建（世界对不上服务器）
  t.test('并发世界快照不覆盖服务器世界种子（joinGame 应用竞态）', async function () {
    var world = {
      v: 4, name: '竞态世界', creative: false, dropMult: 1,
      galaxySeed: 123456, galaxyCount: 1, currentPlanet: 0, dayTime: 0.3,
      planets: { '0': { mods: {}, machines: [], shipPos: [6, 40, 6], seed: 42424242, biome: 'lush' } },
      galaxyArchives: {}, market: {}, mapMarks: {}, flags: {}, warpLock: null,
    };
    var p = Game.joinGame(Object.assign({}, world, { spawn: null, you: null }));
    // 在 genPlanet 的等待窗口内并发执行世界快照（与 newGame 建档同路径）
    Game.buildNetWorld();
    await p;
    A.eq(World.seed, 42424242, 'server seed survives concurrent world snapshot');
  });

  t.test('服务器人物数据应用（you.char）+ 指定出生点', async function () {
    var spawn = { planet: 0, p: [12, 45, 12], st: 'planet', yaw: 1.2 };
    var you = { id: 9, name: '服务器旅行者', char: {
      v: 4, kind: 'char', name: '服务器旅行者',
      appearance: null,
      player: { pos: [12, 45, 12], yaw: 1.2, pitch: 0, stats: { hp: 100, hpMax: 100 }, inv: [{ item: 'iron_ingot', n: 5 }], hotIdx: 0, credits: 88, appearance: null },
      techState: { survival: true }, questIdx: 0, playTime: 10, fuelLoaded: 1,
      playerShip: { model: 'ship', cls: 'C', name: '测试船', inv: Array(12).fill(null) }, shipGarage: [],
    } };
    var world = {
      v: 4, name: '服务器世界2', creative: false, dropMult: 4,
      galaxySeed: 123456, galaxyCount: 1, currentPlanet: 0, dayTime: 0.5,
      planets: { '0': { mods: {}, machines: [], shipPos: [6, 40, 6], seed: 30303030, biome: 'lush' } },
      galaxyArchives: {}, market: {}, mapMarks: {}, flags: {}, warpLock: null,
    };
    await Game.joinGame(Object.assign({}, world, { spawn: spawn, you: you }));
    A.eq(api.credits(), 88, '服务器人物信用点恢复');
    A.eq(api.count('iron_ingot'), 5, '服务器人物背包恢复');
    A.eq(Game.charName, '服务器旅行者', '服务器人物名字应用');
    var p = api.pos();
    A.eq(p[0], 12, '指定出生点 X');
    A.eq(p[2], 12, '指定出生点 Z');
    A.ok(Math.abs(Player.yaw - 1.2) < 0.01, '出生朝向应用');
  });
});

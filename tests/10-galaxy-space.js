/* STARFORGE 测试套件 10 — 星系/太空（种子 + 冒烟进入太空） */
__SF_TEST__.suite('galaxy-space', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('home galaxy seed is origin', function () {
    A.eq(api.galaxySeed(), api.defs.HOME_GALAXY_SEED, 'origin galaxy 7777');
  });

  t.test('space smoke: enter space and inspect planets', function () {
    api.enterSpace();
    var st = api.spaceState();
    A.eq(st.seed, api.defs.HOME_GALAXY_SEED, 'space galaxy seed');
    A.ok(st.planets.length >= 5, 'home system planets >= 5');
    A.ok(st.ship && typeof st.ship.speed === 'number', 'ship state has speed');
    // 每颗星球 def 完整
    for (var i = 0; i < st.planets.length; i++) {
      A.ok(st.planets[i].name, 'planet ' + i + ' named');
      A.ok(api.defs.BIOMES[st.planets[i].biome], 'planet ' + i + ' biome valid');
    }
  });

  // 回归：LOD 溶解 uniform 按星球隔离——A 星球开表皮溶解，不得把 B 星球可见的
  // LOD 地形块沿同一方向压沉（此前模块级共享 lodHoleU，溶解波及所有星球）
  t.test('surface hole uniforms isolated per planet', function () {
    api.enterSpace();
    var ps = window.Space.planets;
    A.ok(ps.length >= 2, 'two planets for isolation check');
    var idA = ps[0].def.id, idB = ps[1].def.id;
    var dir = new THREE.Vector3(0, 1, 0);
    window.Space.setSurfaceHole(idA, 1, dir);
    A.eq(window.Space.debugLodHoleAmt(idA), 1, 'planet A lod hole amt 1');
    A.eq(window.Space.debugLodHoleAmt(idB), 0, 'planet B lod hole unaffected');
    // 换目标星球：其余星球被清除（与皮肤侧 holeU 同语义），各自 uniform 独立互不串扰
    window.Space.setSurfaceHole(idB, 0.5, dir);
    A.eq(window.Space.debugLodHoleAmt(idA), 0, 'A cleared when B becomes target');
    A.eq(window.Space.debugLodHoleAmt(idB), 0.5, 'B own dissolve applied');
    window.Space.setSurfaceHole(-1);
    A.eq(window.Space.debugLodHoleAmt(idA), 0, 'clear-all resets A lod hole');
    A.eq(window.Space.debugLodHoleAmt(idB), 0, 'clear-all resets B lod hole');
  });

  t.test('planet textures: nearest-mip sampling (no far moire)', function () {
    api.enterSpace();
    var ps = window.Space.planets;
    A.ok(ps && ps.length >= 5, 'planets ready');
    for (var i = 0; i < ps.length; i++) {
      var tx = ps[i].tex;
      A.ok(tx, 'planet ' + i + ' has texture');
      A.eq(tx.magFilter, THREE.NearestFilter, 'planet ' + i + ' magFilter stays nearest (pixel look)');
      A.eq(tx.minFilter, THREE.NearestMipmapNearestFilter, 'planet ' + i + ' minFilter = nearest-mip');
      A.ok(tx.generateMipmaps, 'planet ' + i + ' generates mipmaps');
    }
  });

  t.test('星图恒星与跃迁精灵同源：每颗星都可实际跃迁（无幽灵星）', async function () {
    // tpTo 才真正把 Game.state 置为 space（太空主循环在此分支惰性构建跃迁精灵）
    await window.Game.tpTo(0, null, 'space', 'test');
    // 远离所有星球：否则无缝入星会在下一帧把状态拉回 atmo，精灵构建永不执行
    window.Space.shipState.pos.set(5000, 5000, 5000);
    var ns0 = window.Game.neighborSeeds();
    await api.waitUntil(function () { return window.Space.getGalaxySpritePos(ns0[0]) !== null; }, 5000, 50);
    window.UI.openGalaxyMap();
    var labels = document.querySelectorAll('#galMap .g3d-label');
    var ns = window.Game.neighborSeeds();
    // 星图 = 当前星系 + neighborSeeds 全部恒星（起源/当前单独渲染，数量恰好 +1）
    A.eq(labels.length, ns.length + 1, 'galaxy map stars == warp targets + current (got ' + labels.length + ', want ' + (ns.length + 1) + ')');
    // 每颗跃迁目标的精灵都必须存在（锁定后方框/箭头/点火依赖它）
    var missing = [];
    for (var i = 0; i < ns.length; i++){
      if (!window.Space.getGalaxySpritePos(ns[i])) missing.push(ns[i]);
    }
    A.eq(missing.length, 0, 'every warp target has a space sprite (missing ' + missing.length + ')');
    window.UI.closeAll();
  });

  // 回归：大厅平台顶面 y=3 建模横贯 z≤6 的全宽（64×4×20 @ z=-4），
  // 而 floorAt 只把 z≤4 当平台——侧翼走道 |x|≥10、4<z≤6 会把玩家塌到库底 y=0 卡进平台
  t.test('station concourse floor spans z<=6 full width', async function () {
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.set(5000, 5000, 5000);   // 远离星球，防无缝入星抢状态
    A.ok(window.Space.getDock(), 'dock available in space');
    var f = window.Station.debugFloorAt;
    A.eq(f(20, 5), 3, 'side walkway (x=20,z=5) stands on concourse top');
    A.eq(f(-25, 6), 3, 'side walkway (x=-25,z=6) stands on concourse top');
    A.eq(f(30, 4), 3, 'front edge of platform (x=30,z=4)');
    A.eq(f(0, 7), 2, 'central second step (z=7)');
    A.eq(f(0, 9), 1, 'central first step (z=9)');
    A.eq(f(0, 11), 0, 'hangar floor beyond steps');
    A.eq(f(20, 7), 0, 'side strip past platform edge (z=7) is hangar floor');
    A.eq(f(20, 31), 2, 'landing pad area (20,31) height 2');
  });

  // 回归：离开星球时地面 HUD 标记不清——太空态不跑 updateMarkers，矿物/飞船标记
  // 冻结在屏幕上的最后位置（冲向太空仍挂在准星旁）
  t.test('ground HUD markers cleared when leaving planet', async function () {
    await window.Game.tpTo(0, null, 'planet', 'test');
    var sp = api.pos();
    api.setPos(sp[0] + 12, sp[1], sp[2]);   // 离船 >8m：飞船标记在星球上显示
    await api.sleep(200);   // 若干帧 updateMarkers
    var el = document.querySelector('#markers .wmark.ship');
    A.ok(el && el.style.display !== 'none', 'ship marker visible on planet');
    await window.Game.tpTo(0, null, 'space', 'test');
    await api.sleep(200);
    A.eq(el.style.display, 'none', 'ship marker hidden in space (修复前冻结可见)');
  });

  // 回归：大气层 C 扫描不清旧标记——连续扫描同一村庄/遗迹堆叠多个重复 POI 标记
  t.test('repeated atmo scans replace old POI markers (no duplicates)', async function () {
    var pd = api.defs.SYSTEM_PLANETS[0];
    var dir = [0.5, 0.6, 0.6];
    var len = Math.sqrt(dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]);
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.fromArray([
      pd.pos[0] + dir[0] / len * (pd.radius + 40),
      pd.pos[1] + dir[1] / len * (pd.radius + 40),
      pd.pos[2] + dir[2] / len * (pd.radius + 40),
    ]);
    window.Space.shipState.speed = 0;
    await api.waitUntil(function () { return window.Game.state === 'atmo'; }, 30000, 50);
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyC' }));
    var n1 = document.querySelectorAll('#markers .wmark.ore').length;   // 同步计数：扫描在 keydown 内同步完成
    A.ok(n1 > 0, 'first scan found POI markers (' + n1 + ')');
    // 第二次扫描走冷却旁路钩子（冷却按游戏时间计，低帧率下墙钟不可预期）
    window.Game.debugAtmoScanNow();
    var n2 = document.querySelectorAll('#markers .wmark.ore').length;
    A.eq(n2, n1, 'second scan replaces old markers (修复前翻倍), got ' + n2 + ' want ' + n1);
  });

  // 回归：nearestTarget 无 bestD 比较——每个进入 220 的星球都覆盖 best（迭代序最后一个
  // 胜出），空间站又无条件覆盖星球：交互提示报的不是最近的星球。修复：最近者胜
  t.test('nearestTarget picks the nearest body (not the last iterated)', async function () {
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.set(60000, 0, 0);   // 远离真实星球与空间站
    // 两个临时假想天体：A 距 50、B 距 100（迭代序 A 先 B 后——修复前 B 覆盖 A）
    var mk = function (x) { return { mesh: { position: new THREE.Vector3(x, 0, 0) }, def: { id: 999, name: '假想', radius: 0 } }; };
    var fa = mk(60050), fb = mk(60100);
    window.Space.planets.push(fa, fb);
    try {
      var t = window.Space.nearestTarget();
      A.ok(t && t.type === 'planet' && t.dist < 60, 'nearest (50u) wins (got dist ' + (t && t.dist) + ')');
    } finally {
      window.Space.planets.pop(); window.Space.planets.pop();
    }
  });

  // 回归：clearSpaceMarkers 只删 DOM 不清 backing 数组——扫描标记带 120s 有效期，
  // 入星/泊入后返回太空，updateSpaceMarkers 会把「已清除」的标记重新画出来
  t.test('space scan markers cleared on seamless entry (no reappear)', async function () {
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.set(5000, 5000, 5000);
    A.eq(window.Space.spaceScan(), true, 'scan started');
    A.ok(window.Space.getSpaceMarkers().length > 0, 'markers scanned (' + window.Space.getSpaceMarkers().length + ')');
    // 无缝入星：clearSpaceMarkers 被调用（修复后 backing 数组同步清空）
    var pd = api.defs.SYSTEM_PLANETS[0];
    var dir = [0.5, 0.6, 0.6];
    var len = Math.sqrt(dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]);
    window.Space.shipState.pos.fromArray([
      pd.pos[0] + dir[0] / len * (pd.radius + 40),
      pd.pos[1] + dir[1] / len * (pd.radius + 40),
      pd.pos[2] + dir[2] / len * (pd.radius + 40),
    ]);
    window.Space.shipState.speed = 0;
    await api.waitUntil(function () { return window.Game.state === 'atmo'; }, 30000, 50);
    A.eq(window.Space.getSpaceMarkers().length, 0, 'backing array cleared on entry');
    await window.Game.tpTo(0, null, 'space', 'test');
    await api.sleep(200);   // 若干帧 updateSpaceMarkers
    A.eq(document.querySelectorAll('#markers .wmark.ore').length, 0, 'no stale scan markers after re-entry');
  });

  // 回归：星系图打开时飞船照常飞行——W/S/J 输入与飞船模拟在星图后面继续跑，
  // 船漂移/脉冲消耗，甚至 tickWarpAutoJump 白耗曲率电池。修复：面板打开冻结飞行
  t.test('ship frozen while galaxy map open', async function () {
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.set(5000, 5000, 5000);   // 远离星球，防无缝入星
    window.Space.shipState.speed = 60;   // 给初速：修复前星图打开仍会漂移
    var p0 = window.Space.shipState.pos.toArray().slice();
    window.UI.openGalaxyMap();
    A.ok(!document.getElementById('galaxyPanel').classList.contains('hidden'), 'galaxy map open');
    await api.sleep(400);
    var p1 = window.Space.shipState.pos.toArray().slice();
    A.ok(Math.abs(p1[0] - p0[0]) < 0.01 && Math.abs(p1[2] - p0[2]) < 0.01,
      'ship frozen while map open (dx=' + (p1[0] - p0[0]).toFixed(3) + ')');
    window.UI.closeAll();
    // 帧率在 CI/本机差异大：用轮询等飞船真正位移（循环一旦恢复跑帧，位移必现）
    await api.waitUntil(function () {
      var pp = window.Space.shipState.pos;
      return Math.hypot(pp.x - p0[0], pp.z - p0[2]) > 1;
    }, 5000, 50);
    var p2 = window.Space.shipState.pos.toArray().slice();
    A.ok(Math.hypot(p2[0] - p0[0], p2[2] - p0[2]) > 1, 'ship resumes flight after map closed');
  });

  // 回归：再入摩擦特效层只在 atmo 态衰减（reentryT>0 时）。再入中途 E 落地（atmoland→seated）
  // 或传送离开大气后 updateAtmo 不再执行，reentryT 永远不为零——特效层永久卡在屏幕上
  t.test('reentry FX overlay cleared when leaving atmo mid-reentry', async function () {
    var fx = function () { return document.getElementById('reentryFx').classList.contains('show'); };
    var pd = api.defs.SYSTEM_PLANETS[0];
    var dir = [0.5, 0.6, 0.6];
    var len = Math.sqrt(dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]);
    function nearPlanet(){
      window.Space.shipState.pos.fromArray([
        pd.pos[0] + dir[0] / len * (pd.radius + 40),
        pd.pos[1] + dir[1] / len * (pd.radius + 40),
        pd.pos[2] + dir[2] / len * (pd.radius + 40),
      ]);
      window.Space.shipState.speed = 0;
    }
    // —— 路径 1：再入中传送离开大气 ——
    await window.Game.tpTo(0, null, 'space', 'test');
    nearPlanet();
    await api.waitUntil(function () { return window.Game.state === 'atmo'; }, 30000, 50);
    A.ok(fx(), 'reentry fx shown on seamless entry');
    window.Game.tpTo(0, null, 'planet', 'test');   // loading 期间 reentryT 冻结，落回 planet 后必须清特效
    await api.waitUntil(function () { return window.Game.state === 'planet'; }, 30000, 50);
    await api.sleep(150);
    A.ok(!fx(), 'reentry fx cleared after leaving atmo (teleport path)');
    // —— 路径 2：再入中途直接按 E 落地（真实玩家路径）——
    await window.Game.tpTo(0, null, 'space', 'test');
    nearPlanet();
    await api.waitUntil(function () { return window.Game.state === 'atmo'; }, 30000, 50);
    A.ok(fx(), 'reentry fx shown on second entry');
    // 找一块非液体着陆点：liquid 顶部会拒绝降落（状态留在 atmo）
    var lp = null, cx = Math.floor(window.Space.shipGroup.position.x), cz = Math.floor(window.Space.shipGroup.position.z);
    for (var r = 0; r <= 60 && !lp; r++){
      for (var dx = -r; dx <= r && !lp; dx++){
        for (var dz = -r; dz <= r && !lp; dz++){
          if (Math.max(Math.abs(dx), Math.abs(dz)) !== r) continue;
          var gy = window.World.topAt(cx + dx, cz + dz);
          if (!window.World.getDef(cx + dx, gy, cz + dz).liquid) lp = [cx + dx, cz + dz];
        }
      }
    }
    A.ok(lp, 'land column found near entry point');
    window.Space.shipGroup.position.x = lp[0] + 0.5;
    window.Space.shipGroup.position.z = lp[1] + 0.5;
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyE' }));
    await api.waitUntil(function () { return window.Game.state === 'seated'; }, 20000, 50);
    await api.sleep(150);
    A.ok(!fx(), 'reentry fx cleared after landing (E path)');
  });

  // 回归：座舱（seated）内生物 AI 继续运行，守卫无人机隔机身攻击玩家并致死——
  // 死亡复活把 Player.pos 送回出生点，而状态/相机仍锁在座舱：状态机互相矛盾
  t.test('seated cockpit is safe from sentinel (creature AI paused in cockpit)', async function () {
    var pd = api.defs.SYSTEM_PLANETS[0];
    var dir = [0.5, 0.6, 0.6];
    var len = Math.sqrt(dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]);
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.fromArray([
      pd.pos[0] + dir[0] / len * (pd.radius + 40),
      pd.pos[1] + dir[1] / len * (pd.radius + 40),
      pd.pos[2] + dir[2] / len * (pd.radius + 40),
    ]);
    window.Space.shipState.speed = 0;
    await api.waitUntil(function () { return window.Game.state === 'atmo'; }, 30000, 50);
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyE' }));
    await api.waitUntil(function () { return window.Game.state === 'seated'; }, 20000, 50);
    // 在攻击半径内生成守卫（落点：Player.pos 在降落时被置为 shipPos+2.2，就在机舱旁）
    var s = window.Creatures.debugSpawnSentinel(window.Player.pos.x + 2, window.Player.pos.z);
    api.setStat('shield', 0);
    api.setStat('hp', 40);
    await api.sleep(4000);
    A.eq(api.stats().hp, 40, 'seated player untouched by sentinel (hp=' + api.stats().hp + ')');
    window.Creatures.kill(s);
  });

  // 回归：lockPointer 白名单漏掉 seated/atmoland——降落完成时的 lockPointer() 请求被吞，
  // 起飞进入 atmo 后指针未锁定，鼠标转向失效直到再点一次画面
  t.test('pointer lock allowed in cockpit (seated) state', async function () {
    var pd = api.defs.SYSTEM_PLANETS[0];
    var dir = [0.5, 0.6, 0.6];
    var len = Math.sqrt(dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]);
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.fromArray([
      pd.pos[0] + dir[0] / len * (pd.radius + 40),
      pd.pos[1] + dir[1] / len * (pd.radius + 40),
      pd.pos[2] + dir[2] / len * (pd.radius + 40),
    ]);
    window.Space.shipState.speed = 0;
    await api.waitUntil(function () { return window.Game.state === 'atmo'; }, 30000, 50);
    A.eq(window.Game.debugLockAllowed(), true, 'atmo allows pointer lock');
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyE' }));
    await api.waitUntil(function () { return window.Game.state === 'seated'; }, 20000, 50);
    A.eq(window.Game.debugLockAllowed(), true, 'seated allows pointer lock (fix: 起飞后鼠标可转向)');
  });

  // 回归：贴近日面反弹分支曾覆写共享 _fwd 为径向向量——同帧速度 >150 时脉冲星流线
  // 沿径向而非航向喷射。改用独立临时向量后，_fwd 保持机头朝向
  t.test('near-sun bounce keeps _fwd aligned to heading', function () {
    api.enterSpace();
    var S = window.Space;
    S.shipState.pitch = 0; S.shipState.yaw = 0; S.shipState.roll = 0;
    S.shipState.speed = 200;   // >150：本帧脉冲星流线会读取 _fwd
    // 正上方贴近日面：进入反弹分支（距离 SUN_R+20 < SUN_R+40）
    S.shipState.pos.copy(S.SUN_POS).add(new THREE.Vector3(0, S.SUN_R + 20, 0));
    S.update(1 / 60, new THREE.PerspectiveCamera(), {
      mouseDX: 0, mouseDY: 0, thrust: false, brake: false, boost: false,
      pulse: false, rollLeft: false, rollRight: false,
    });
    var f = S.debugFwd();
    A.ok(Math.abs(f.y) < 0.2, 'forward stays on heading plane after sun bounce (y=' + f.y.toFixed(2) + ')');
    A.ok(f.z < -0.8, 'forward points -Z per heading (z=' + f.z.toFixed(2) + ')');
  });

  // 回归：站内对话归属发起 NPC——走近另一个站员时对话必须结束，否则可以
  // 「对着 B 结算 A 的对话」（购船对话同理：走到别的驾驶员旁仍能成交 A 的船）
  t.test('station dialog ends when switching to another NPC', async function () {
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.set(5000, 5000, 5000);   // 远离星球，防无缝入星
    var dk = window.Space.getDock();
    A.ok(dk && dk.staff.length >= 3, 'staff for owner switch');
    // staff[0] 站在交易终端旁（<4.2 会判 near='terminal'）；用大厅两侧的站务长/领航员
    var a = dk.staff[1], b = dk.staff[2];
    var cam = new THREE.PerspectiveCamera();
    // 走到 A 旁 → 开对话（归属 A）
    var wa = a.position.clone().add(dk.origin);
    window.Station.debugWalkTo(wa.x, wa.y, wa.z);
    window.Station.update(1 / 60, cam, false, { mouseDX: 0, mouseDY: 0 });
    window.Station.pressE();
    A.ok(window.Station.dialogOpen, 'dialog open with owner A');
    // 走到 B 旁：near 变为 B ≠ 归属 A → 对话结束（修复前仍开着）
    var wb = b.position.clone().add(dk.origin);
    window.Station.debugWalkTo(wb.x, wb.y, wb.z);
    window.Station.update(1 / 60, cam, false, { mouseDX: 0, mouseDY: 0 });
    A.ok(!window.Station.dialogOpen, 'dialog closed when switching NPC (was A, now B)');
  });

  // 回归：站内对话纳入全局 dialogActive/Esc/点击——Esc 应关闭对话而非开暂停面板，
  // 点击画面应推进打字机（此前站内 dlg 与 main 的 dlg 互不感知）
  t.test('station dialog responds to Esc and click via global dialog system', async function () {
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.set(5000, 5000, 5000);   // 远离星球，防无缝入星
    var dk = window.Space.getDock();
    var a = dk.staff[1];   // 站务长（大厅侧翼，离终端/船/换船电脑都远）
    var wa = a.position.clone().add(dk.origin);
    window.Station.debugWalkTo(wa.x, wa.y, wa.z);
    window.Station.update(1 / 60, new THREE.PerspectiveCamera(), false, { mouseDX: 0, mouseDY: 0 });
    window.Station.pressE();
    A.ok(window.Station.dialogOpen, 'dialog open');
    // Esc：关闭站内对话，而不是打开暂停面板（修复前 dialogActive 恒 false → 开暂停面板）
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'Escape' }));
    A.ok(!window.Station.dialogOpen, 'Esc closes station dialog');
    A.ok(document.getElementById('pausePanel').classList.contains('hidden'), 'Esc did not open pause panel');
    // 再开对话，打字机未显完时点击 → 立即补完当前句（修复前点击只锁定指针不推进）
    window.Station.pressE();
    A.ok(window.Station.dialogOpen, 'dialog reopened');
    var before = window.Station.debugDlgChars();
    document.dispatchEvent(new MouseEvent('mousedown', { button: 0 }));
    var after = window.Station.debugDlgChars();
    A.ok(after > before, 'click advances station dialog (chars ' + before + ' -> ' + after + ')');
    window.Station.closeDialog();
  });

  // 回归：跃迁完成块把 warpAnim 置 null 而 state 仍为 warping——下一帧解引用崩溃，
  // catch 兜底以 seed=0 错误跃迁（跳到 0 号星系），200ms 白闪超时又二次 finishWarp（双重抵达）
  t.test('warp arrival lands on the locked galaxy (no null-warpAnim crash)', async function () {
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.set(5000, 5000, 5000);   // 远离星球，防无缝入星
    api.give('warpcell', 5);
    var target = window.Game.neighborSeeds()[0];
    window.Game.warpTo(target);
    A.eq(window.Game.state, 'warping', 'warp engaged');
    // 等待跃迁动画 + 白闪完成（修复前 catch 兜底提前跳到 seed 0 星系）
    await api.waitUntil(function () { return window.Game.state === 'space'; }, 40000, 100);
    A.eq(api.galaxySeed(), target, 'arrived at locked galaxy (got ' + api.galaxySeed() + ', want ' + target + ')');
    await api.sleep(600);   // 白闪超时窗口后：不允许二次 finishWarp 跳走
    A.eq(api.galaxySeed(), target, 'no second arrival (seed stable)');
    // 复原：跃迁回起源星系（后续用例依赖母星系星球布局）
    window.Game.warpTo(777);
    await api.waitUntil(function () { return window.Game.state === 'space'; }, 40000, 100);
    await api.sleep(600);
    A.eq(api.galaxySeed(), 777, 'returned to home galaxy');
  });

  // 回归：无缝入星必须初始化目标星球的生态世界（此前区块快照缓存永不落 {map}，
  // prepPlanet 永远跳过 World.init → 所有星球进入大气后都沿用上一颗星球的世界）
  t.test('seamless atmosphere entry loads target planet biome', async function () {
    for (var i = 0; i < [1, 2, 3, 4].length; i++) {
      var pid = [1, 2, 3, 4][i];
      var pd = api.defs.SYSTEM_PLANETS[pid];
      window.Game.tpTo(pid, null, 'space', 'test');
      // 飞船直接放进目标星球大气握手高度内（表面上方 40 单位）
      var dir = [0.5, 0.6, 0.6];
      var len = Math.sqrt(dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]);
      window.Space.shipState.pos.fromArray([
        pd.pos[0] + dir[0] / len * (pd.radius + 40),
        pd.pos[1] + dir[1] / len * (pd.radius + 40),
        pd.pos[2] + dir[2] / len * (pd.radius + 40),
      ]);
      window.Space.shipState.speed = 0;
      await api.waitUntil(function () { return window.Game.state === 'atmo'; }, 30000, 50);
      A.eq(window.Game.currentPlanet, pid, 'currentPlanet = ' + pid);
      A.eq(window.World.biome.key, pd.biome, 'planet ' + pid + ' world biome = ' + pd.biome);
      A.ne(window.World.biome.key, 'lush', 'planet ' + pid + ' world biome not stuck at lush');
    }
    await api.reboot('normal');   // 恢复干净的星球 0 状态，避免污染后续套件
  });
});

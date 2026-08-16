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

/* STARFORGE 测试套件 20 — 飞行修复回归
   1) 区块淡入保持地形不透明（跨越区块地形不再「消失→突变」，湖床不再透视到地底）
   2) 大气飞船真实方块碰撞（不再强制跳到列顶上方，可进入上方有方块的地方）
   3) 高速入星握手夹紧（交棒期间不再整颗穿过星球）
   4) 大气层内开火（弹道挂体素飞船/星球场景并持续推进） */
__SF_TEST__.suite('flightfixes', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('creative', { fresh: true, seed: 424242 }); });

  // ---- 工具 ----
  function enterAtmo0(){
    return window.Game.tpTo(0, null, 'space', 'test').then(async function () {
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
      window.Game.atmo.speed = 0;   // 悬停（速度归零防漂移干扰断言）
      await api.sleep(400);
    });
  }
  function findShipMesh(){
    var out = null;
    window.Game.planetScene.traverse(function (o){ if (o.userData && o.userData.beacon) out = o; });
    return out;
  }
  function findCamera(){
    var out = null;
    window.Game.planetScene.traverse(function (o){ if (o.isCamera) out = o; });
    return out;
  }

  // 回归：区块淡入曾用透明度淡入——低于 alphaTest 阈值的帧整块被丢弃：跨越区块时新地块
  // 凭空消失（远景模拟地形顶替，地形看似全变）、水面先于湖床可见（液体底部直接透视到地底）。
  // 修复后实心地形亮度淡入（opacity 恒 1，alphaTest 永不丢弃），湖床全程垫底。
  t.test('chunk fade-in keeps lake bed opaque (no see-through to void)', async function () {
    var p = api.pos();
    var x0 = Math.floor(p[0]) + 40, z0 = Math.floor(p[2]) + 40;   // 偏移避开出生点结构
    // 人造湖：红藓床 y=16 + 水 17..20 + 四周石墙（挖空到 y=40 保证视野干净）
    for (var dx = -14; dx <= 14; dx++){
      for (var dz = -14; dz <= 14; dz++){
        var rim = Math.abs(dx) === 14 || Math.abs(dz) === 14;
        for (var y = 0; y <= (rim ? 30 : 16); y++) api.setBlock(x0 + dx, y, z0 + dz, rim ? 'stone' : 'redmoss');
        if (!rim) for (var y2 = 17; y2 <= 40; y2++) api.setBlock(x0 + dx, y2, z0 + dz, 'air');
      }
    }
    for (var wx = -12; wx <= 12; wx++)
      for (var wz = -12; wz <= 12; wz++)
        for (var wy = 17; wy <= 20; wy++) api.setBlock(x0 + wx, wy, z0 + wz, 'water');
    api.setPos(x0 + 0.5, 30, z0 + 0.5);
    window.Player.pitch = -1.4; window.Player.yaw = 0;
    await api.waitUntil(function () { return window.World.stats().pending === 0; }, 30000, 100);
    await api.sleep(1200);   // 等首轮淡入结束
    // 远传送卸载网格（改过的区块数据保留、网格剔除）再回来 → 触发重新淡入
    api.setPos(x0 + 900, 30, z0);
    await api.sleep(1200);
    api.setPos(x0 + 0.5, 30, z0 + 0.5);
    window.Player.pitch = -1.4; window.Player.yaw = 0;
    var cx = Math.floor(x0 / 16), cz = Math.floor(z0 / 16);
    await api.waitUntil(function () {
      var f = window.World.debugChunkFlags(cx, cz);
      return !!(f && f.mesh);
    }, 30000, 50);
    // 淡入全程逐帧断言：实心地形必须亮度淡入（opacity 恒 1）——透明度淡入低于 alphaTest
    // 阈值时被整块丢弃，湖床先于水面消失（修复前固体淡入条目 opacity < 1 即触发透视）
    var fadeSeen = false, solidOpaque = true;
    for (var i = 0; i < 14; i++){
      var fades = window.World.debugFadeIns;
      for (var k = 0; k < fades.length; k++){
        fadeSeen = true;
        var f = fades[k];
        if (f.solid && f.opacity !== 1) solidOpaque = false;
      }
      await api.sleep(80);
    }
    A.ok(fadeSeen, 'chunk fade-in observed after return');
    A.ok(solidOpaque, 'solid chunk fade keeps opacity 1 (brightness fade, alphaTest never discards terrain)');
  });

  // 回归：飞船曾被列顶高度强制抬到方块上方——上方有方块（悬空平台/洞穴顶）的地方进不去。
  // 修复后按真实方块碰撞推挤：平台悬在头顶时飞船原地悬停，绝不跳上平台。
  t.test('atmo ship stays below overhanging platform (no teleport above blocks)', async function () {
    await enterAtmo0();
    var ship = findShipMesh();
    A.ok(ship, 'flying ship mesh found');
    var sx = Math.floor(ship.position.x), sz = Math.floor(ship.position.z);
    var py = Math.floor(ship.position.y) + 6;   // 平台悬在船上方 6 格
    for (var dx = -4; dx <= 4; dx++)
      for (var dz = -4; dz <= 4; dz++)
        for (var y = py; y <= py + 2; y++) api.setBlock(sx + dx, y, sz + dz, 'stone');
    var y0 = ship.position.y;
    await api.sleep(1600);
    var y1 = ship.position.y;
    A.ok(Math.abs(y1 - y0) < 1.2, 'ship stays under the platform (y ' + y0.toFixed(1) + ' -> ' + y1.toFixed(1) + ', fix: 不再强制跳到方块上方)');
    A.ok(y1 < py, 'ship remains below the platform top (' + y1.toFixed(1) + ' < ' + (py + 2) + ')');
  });

  // 回归：大气内点击开火无路由——mousedown 只在 space 态调 Space.shoot，且 shoot 用太空坐标
  // 枪口/太空场景，大气态完全无法开火。修复后 atmo 态可开火，弹道挂体素飞船并持续推进。
  t.test('ship can fire inside atmosphere (bolt in planet scene)', async function () {
    await enterAtmo0();
    var ship = findShipMesh();
    A.ok(ship, 'flying ship mesh found');
    A.eq(window.Game.debugShipFireAllowed(), true, 'atmo routes clicks to ship fire');
    var cam = findCamera();
    A.ok(cam, 'camera found in planet scene');
    var n0 = window.Space.debugLasers.length;
    window.Space.shoot(cam);
    var ls = window.Space.debugLasers;
    A.eq(ls.length, n0 + 1, 'one bolt fired');
    var b = ls[ls.length - 1];
    A.eq(b.space, false, 'bolt belongs to planet scene (atmo bolt)');
    var sp = ship.position;
    A.ok(Math.abs(b.pos[0] - sp.x) < 12 && Math.abs(b.pos[2] - sp.z) < 12,
      'bolt spawned at flying ship (got [' + b.pos.map(function (v){ return v.toFixed(1); }).join(',') + '])');
    // 弹道推进（轮询等待位移：低帧率 CI 下固定 sleep 会竞态）
    await api.waitUntil(function () {
      var l = window.Space.debugLasers;
      if (!l.length) return false;
      var nb = l[l.length - 1];
      return Math.abs(nb.pos[0] - b.pos[0]) + Math.abs(nb.pos[2] - b.pos[2]) > 1;
    }, 5000, 50);
    var b2 = window.Space.debugLasers[window.Space.debugLasers.length - 1];
    A.ok(Math.abs(b2.pos[0] - b.pos[0]) + Math.abs(b2.pos[2] - b.pos[2]) > 1, 'bolt advances inside atmosphere');
  });

  // 回归：高速进入大气直接穿过整个星球——入星交接（区块快照异步加载）期间飞船仍按帧
  // 推进，脉冲极速下交棒未完成就已穿球。修复后交接期间逐帧把船夹在握手球面上。
  t.test('fast approach clamped at handoff sphere while entry pending', async function () {
    await window.Game.tpTo(0, null, 'space', 'test');
    var pd = api.defs.SYSTEM_PLANETS[0];
    var s = pd.radius * 0.004;
    var hd = (150 - window.World.SEA_Y) * s;
    // 船放在握手高度下方 60u、正冲行星中心高速飞行
    window.Space.shipState.pos.fromArray([pd.pos[0], pd.pos[1], pd.pos[2] + pd.radius + hd - 60]);
    window.Space.shipState.pitch = 0; window.Space.shipState.yaw = 0; window.Space.shipState.roll = 0;
    window.Space.shipState.speed = 900;
    window.Game.debugSetLandingLock(true);   // 模拟入星交接（区块快照加载）进行中
    await api.sleep(500);
    var pos = window.Space.shipState.pos;
    var dxp = pos.x - pd.pos[0], dyp = pos.y - pd.pos[1], dzp = pos.z - pd.pos[2];
    var d = Math.sqrt(dxp * dxp + dyp * dyp + dzp * dzp) - pd.radius;
    A.ok(d >= hd - 2, 'ship clamped at handoff sphere (d=' + d.toFixed(1) + ', hd=' + hd.toFixed(1) + '; fix: 不再穿过星球)');
    window.Game.debugSetLandingLock(false);
    window.Space.shipState.pos.set(5000, 5000, 5000);   // 移走，防后续用例触发入星
    window.Space.shipState.speed = 0;
  });

  t.after(function () { return api.reboot('normal'); });   // 恢复干净世界，避免污染后续套件
});

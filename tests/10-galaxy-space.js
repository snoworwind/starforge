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

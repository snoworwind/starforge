/* STARFORGE 测试套件 18 — 液体物理（浮力/阻力/游泳）与熔岩接触伤害 */
__SF_TEST__.suite('liquid', function (t, api) {
  var A = api.assert;
  var CAM = {
    position: new THREE.Vector3(),
    quaternion: new THREE.Quaternion(),
    getWorldDirection: function (v) { v.set(0, 0, -1); return v; },
    updateMatrixWorld: function () {},
    add: function () {}, remove: function () {},
  };
  function step(n) { for (var i = 0; i < n; i++) window.Player.update(1 / 60, CAM); }
  // 在地面 (x,z) 上方建 6 层深 3×3 水池/熔岩池（返回 [x, gy+1.05, z, gy]）
  function makePool(x, z) {
    var gy = api.topAt(x, z);
    var dx, dz, y;
    for (dx = -1; dx <= 1; dx++) for (dz = -1; dz <= 1; dz++)
      for (y = 1; y <= 6; y++) api.setBlock(x + dx, gy + y, z + dz, 'water');
    return [x, gy + 1.05, z, gy];
  }
  function clearPool(x, z, gy) {
    var dx, dz, y;
    for (dx = -1; dx <= 1; dx++) for (dz = -1; dz <= 1; dz++)
      for (y = 1; y <= 6; y++) api.setBlock(x + dx, gy + y, z + dz, 'air');
  }

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('水中浮力制动坠落并缓慢上浮', function () {
    var X = 70, Z = 70;
    var pool = makePool(X, Z);
    document.getElementById('pausePanel').classList.remove('hidden');
    api.setPos(pool[0] + 0.5, pool[1], pool[2] + 0.5);
    window.Player.vel.y = -10;   // 模拟坠入水中
    window.Player.keys['Space'] = false;
    step(30);   // 0.5s
    var v1 = window.Player.vel.y;
    A.ok(v1 > -3, 'buoyancy arrests the fall (v=' + v1.toFixed(2) + ')');
    step(30);   // 再 0.5s：浮力趋近 +2.6
    A.ok(window.Player.vel.y > 0.5, 'rises toward surface (v=' + window.Player.vel.y.toFixed(2) + ')');
    window.Player.vel.set(0, 0, 0);
    document.getElementById('pausePanel').classList.add('hidden');
    clearPool(X, Z, pool[3]);
  });

  t.test('按住空格游泳上浮', function () {
    var X = 80, Z = 80;
    var pool = makePool(X, Z);
    document.getElementById('pausePanel').classList.remove('hidden');
    api.setPos(pool[0] + 0.5, pool[1], pool[2] + 0.5);
    window.Player.vel.set(0, 0, 0);
    window.Player.keys['Space'] = true;
    step(30);   // 0.5s
    var v = window.Player.vel.y;
    A.ok(v > 2, 'swimming up (v=' + v.toFixed(2) + ')');
    A.ok(api.pos()[1] > pool[1], 'player gained height in water');
    window.Player.keys['Space'] = false;
    window.Player.vel.set(0, 0, 0);
    document.getElementById('pausePanel').classList.add('hidden');
    clearPool(X, Z, pool[3]);
  });

  t.test('熔岩湖接触灼烧造成伤害', function () {
    var X = 90, Z = 90;
    var pool = makePool(X, Z);
    document.getElementById('pausePanel').classList.remove('hidden');
    api.setPos(pool[0] + 0.5, pool[1], pool[2] + 0.5);
    api.setStat('shield', 0);
    api.setStat('o2', 10);   // 阻断护盾回复（o2>20 才回复），防止 0.0025 的小数护盾吸掉整点伤害
    api.setStat('hp', 50);
    var savedLava = window.World.biome.lava;
    window.World.biome.lava = true;   // 模拟熔火之地的熔岩水
    var feetKey = api.blockKeyAt(pool[0] + 0.5, pool[1] + 0.15, pool[2] + 0.5);
    step(70);   // 70 帧 @ 3 伤害/秒（60 帧因浮点累积≈2.9999 只够 2 次伤害，留余量）
    var hp = api.stats().hp;
    var y = api.pos()[1];
    var feetKey2 = api.blockKeyAt(pool[0] + 0.5, y + 0.15, pool[2] + 0.5);
    A.ok(hp <= 47, 'lava burns player (hp=' + hp + ', y=' + y.toFixed(2) + ', feetBlock=' + feetKey + '→' + feetKey2 + ', poolBottom=' + pool[3] + ')');
    window.World.biome.lava = savedLava;
    document.getElementById('pausePanel').classList.add('hidden');
    clearPool(X, Z, pool[3]);
  });

  t.test('掉落物脚下被挖空后继续坠落', function () {
    // 平台悬在真实地面上方：掉落物落在平台上 → 拆除平台 → 应继续坠到地面
    var X = 100, Z = 100;
    var gy = api.topAt(X, Z);
    api.setBlock(X, gy + 1, Z, 'dirt');   // 平台
    document.getElementById('pausePanel').classList.remove('hidden');   // 冻结世界，避免主循环拾取/干扰
    window.Player.spawnDrop(X + 0.5, gy + 2.5, Z + 0.5, 'stone', 1, null, 999);
    var i;
    for (i = 0; i < 40; i++) window.Player.update(1 / 30, CAM);   // 落地（悬浮在平台上方）
    var y0 = window.Player.debugLastDropY();
    A.ok(y0 !== null && y0 > gy + 1.5, 'drop resting on platform (y=' + (y0 === null ? 'null' : y0.toFixed(2)) + ')');
    api.setBlock(X, gy + 1, Z, 'air');   // 挖掉平台
    for (i = 0; i < 40; i++) window.Player.update(1 / 30, CAM);
    var y1 = window.Player.debugLastDropY();
    A.ok(y1 !== null && y1 < y0 - 0.5, 'drop fell after ground mined (y0=' + y0.toFixed(2) + ' → y1=' + (y1 === null ? 'null' : y1.toFixed(2)) + ')');
    document.getElementById('pausePanel').classList.add('hidden');
  });

  t.test('眼部液体检测按摄像机高度（1.62 而非 1.2）', function () {
    // 悬浮水层：地面 gy、空气 gy+1、水 gy+2。玩家脚底放在 gy+0.6 处时，
    // 眼块（+1.62 → gy+2.22）在水中，而旧检测高度（+1.2 → gy+1.8）在空气里
    var X = 120, Z = 120;
    var gy = api.topAt(X, Z);
    api.setBlock(X, gy + 2, Z, 'water');
    document.getElementById('pausePanel').classList.remove('hidden');
    api.setPos(X + 0.5, gy + 0.6, Z + 0.5);
    A.eq(window.Player.debugEyeInLiquid(), true, 'eye submerged in water detected');
    api.setPos(X + 0.5, gy + 1.05, Z + 0.5);
    A.eq(window.Player.debugEyeInLiquid(), true, 'standing under water layer still detected');
    api.setBlock(X, gy + 2, Z, 'air');
    document.getElementById('pausePanel').classList.add('hidden');
  });
});

/* STARFORGE 测试套件 05 — 体素世界（确定性生成 + 方块读写 + 射线 + 序列化） */
__SF_TEST__.suite('world', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true, seed: 12345 }); });

  t.test('booted into planet with deterministic seed', function () {
    A.eq(api.state(), 'planet', 'state is planet');
    A.ok(api.worldSeed() > 0, 'world seed positive');
    A.ok(api.biome() != null, 'biome set');
    A.ok(api.snapshot().planetName != null, 'planet named');
  });

  t.test('setBlock / blockKeyAt roundtrip', function () {
    var x = 60, z = 60, y = Math.min(api.topAt(x, z) + 1, 95);
    api.setBlock(x, y, z, 'stone');
    A.eq(api.blockKeyAt(x, y, z), 'stone', 'stone placed');
    api.setBlock(x, y, z, 'air');
    A.eq(api.blockKeyAt(x, y, z), 'air', 'back to air');
  });

  t.test('topAt within world height', function () {
    A.between(api.topAt(0, 0), 0, 95, 'height 0..95');
    A.between(api.topAt(-120, 340), 0, 95, 'height far 0..95');
  });

  t.test('findSpawn on non-liquid ground', function () {
    var p = api.findSpawn();
    A.ok(p[1] > 0, 'spawn above zero');
  });

  t.test('raycast hits ground downward', function () {
    var x = 50, z = 50, gy = api.topAt(x, z);
    var hit = api.raycast([x + 0.5, gy + 10, z + 0.5], [0, -1, 0], 40);
    A.ok(hit, 'raycast hit');
    A.ok(hit.def && hit.def.id !== 0, 'hit a solid block');
  });

  t.test('deterministic world across reboot (same seed)', function () {
    var s1 = api.worldSeed();
    var h1 = api.topAt(100, 100);
    var sp1 = window.World.findSpawn();
    return api.reboot('normal', { seed: 12345 }).then(function () {
      A.eq(api.worldSeed(), s1, 'same world seed');
      A.eq(api.topAt(100, 100), h1, 'same terrain height');
      var sp2 = window.World.findSpawn();
      A.ok(Math.abs(sp1.x - sp2.x) < 0.01 && Math.abs(sp1.z - sp2.z) < 0.01, 'findSpawn deterministic (seed-derived)');
    });
  });

  t.test('World.serialize returns seed and mods', function () {
    var s = window.World.serialize();
    A.ok(s.seed != null, 'serialize seed');
    A.ok(typeof s.mods === 'object', 'serialize mods object');
  });

  t.test('建材方块：半砖（半高碰撞）/金属块/混凝土块', function () {
    var x = 64, z = 64, y = Math.min(api.topAt(x, z) + 1, 95);
    // 半砖：数值 lowbox = 0.45（可站上的半高块）
    api.setBlock(x, y, z, 'slab');
    A.eq(api.blockKeyAt(x, y, z), 'slab', 'slab placed');
    var d = window.World.getDef(x, y, z);
    A.eq(d.lowbox, 0.45, 'slab lowbox height 0.45');
    A.ok(d.solid, 'slab is solid');
    api.setBlock(x, y, z, 'air');
    // 金属块
    api.setBlock(x, y, z, 'metal');
    A.eq(api.blockKeyAt(x, y, z), 'metal', 'metal placed');
    A.eq(window.World.getDef(x, y, z).tiles.all, 'metal', 'metal tile');
    api.setBlock(x, y, z, 'air');
    // 混凝土块
    api.setBlock(x, y, z, 'concrete');
    A.eq(api.blockKeyAt(x, y, z), 'concrete', 'concrete placed');
    A.eq(window.World.getDef(x, y, z).tiles.all, 'concrete', 'concrete tile');
    api.setBlock(x, y, z, 'air');
  });

  t.test('stream 生成预算：单帧含邻块不超过 4，多帧后视距内无破洞', function () {
    var p = api.pos();
    // 传送 512 格外（远超视距）：旧区块全部卸载，触发全新流式生成
    api.setPos(p[0] + 512, p[1], p[2]);
    var g0 = window.World.genCount;
    window.World.stream(api.pos()[0], api.pos()[2]);
    var d1 = window.World.genCount - g0;
    // 修复前：网格化时无视预算同步生成最多 8 个邻块 → 单帧可达 12 次 genChunk
    A.ok(d1 <= 4, '单帧生成（含邻块）不超过预算 4，实际 ' + d1);
    // 迭代足够多帧后收敛：视距内区块全部网格化，没有被预算永久跳过的破洞
    var i;
    for (i = 0; i < 300; i++) window.World.stream(api.pos()[0], api.pos()[2]);
    var st = window.World.stats();
    A.ok(st.chunks > 0, '区块已生成, stats=' + JSON.stringify(st));
    A.eq(st.pending, 0, '视距内无待网格化区块（破洞=0）, stats=' + JSON.stringify(st));
    A.ok(st.meshed > 0, '视距内已有网格化区块, stats=' + JSON.stringify(st));
  });

  // 回归：区块数据从不剔除——玩家沿一个方向探索，内存以 ~24KB/块 无界增长。
  // 修复：超出卸载半径+6 格、未被机器占用、未改动、无待落盘的区块整体删除
  t.test('远场未改动区块数据剔除（内存有界）', async function () {
    var sp = api.pos();
    var kx = Math.floor(sp[0] / 16), kz = Math.floor(sp[2] / 16);
    // 让主循环跑几帧：出生区块落盘完成（needSave=false 才可剔除）
    for (var i = 0; i < 30; i++) window.World.stream(sp[0], sp[2]);
    await api.sleep(300);
    A.ok(window.World.debugHasChunk(kx, kz), 'spawn chunk in memory');
    // 传送 800m（远超 UNLOAD_R+6≈400m）：流式扫描在远场生成、近场卸载
    var fx = sp[0] + 800, fz = sp[2] + 800;
    api.setPos(fx, sp[1] + 30, fz);
    for (var j = 0; j < 300; j++){ window.World.stream(fx, fz); window.World.update(1 / 30, fx, fz); }
    await api.sleep(200);
    A.ok(!window.World.debugHasChunk(kx, kz), 'distant pristine chunk evicted (内存有界)');
    // 返回出生点：数据由程序化地形确定性还原
    api.setPos(sp[0], sp[1], sp[2]);
    for (var k = 0; k < 300; k++){ window.World.stream(sp[0], sp[2]); window.World.update(1 / 30, sp[0], sp[2]); }
    await api.sleep(200);
    A.ok(window.World.debugHasChunk(kx, kz), 'spawn chunk regenerated on return');
  });
});

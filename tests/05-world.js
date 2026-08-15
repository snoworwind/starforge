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
    return api.reboot('normal', { seed: 12345 }).then(function () {
      A.eq(api.worldSeed(), s1, 'same world seed');
      A.eq(api.topAt(100, 100), h1, 'same terrain height');
    });
  });

  t.test('World.serialize returns seed and mods', function () {
    var s = window.World.serialize();
    A.ok(s.seed != null, 'serialize seed');
    A.ok(typeof s.mods === 'object', 'serialize mods object');
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
});

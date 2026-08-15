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
});

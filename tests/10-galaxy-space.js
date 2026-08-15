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
});

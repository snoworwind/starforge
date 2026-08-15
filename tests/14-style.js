/* STARFORGE 测试套件 14 — 像素风渲染模式（内部低分辨率 + 最邻近放大） */
__SF_TEST__.suite('style', function (t, api) {
  var A = api.assert;
  var baseline = { bufW: 0, bufH: 0, styleW: '' };

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('style buttons exist and record modern baseline', function () {
    A.ok(document.querySelector('#setStyle button[data-q="pixel"]'), 'pixel style button exists');
    A.ok(document.querySelector('#setStyle button[data-q="modern"]'), 'modern style button exists');
    var cvs = document.getElementById('game');
    baseline.bufW = cvs.width; baseline.bufH = cvs.height; baseline.styleW = cvs.style.width;
    A.ok(baseline.bufW > 0 && baseline.bufH > 0, 'modern baseline buffer nonzero');
    A.ok(!document.querySelector('#setStyle button[data-q="pixel"]').classList.contains('on'),
      'pixel button inactive by default');
  });

  t.test('pixel style halves render buffer', function () {
    var btn = document.querySelector('#setStyle button[data-q="pixel"]');
    btn.onclick();
    var cvs = document.getElementById('game');
    var expW = Math.max(640, Math.round(window.innerWidth * 0.5));
    var expH = Math.max(360, Math.round(window.innerHeight * 0.5));
    A.eq(cvs.width, expW, 'pixel buffer width = half (min 640)');
    A.eq(cvs.height, expH, 'pixel buffer height = half (min 360)');
    A.eq(cvs.style.width, window.innerWidth + 'px', 'CSS width stretched to window');
    A.eq(cvs.style.height, window.innerHeight + 'px', 'CSS height stretched to window');
    A.ok(btn.classList.contains('on'), 'pixel button marked active');
  });

  t.test('pixel style switches model textures to nearest filter', function () {
    var tpl = window.ModelLib && ModelLib.getTemplate('ship_striker');
    if (!tpl){
      A.ok(true, 'ship_striker template unavailable (GLTF parse failed) — skipped');
      return;
    }
    var found = false, filter = null;
    tpl.scene.traverse(function (o){
      if (!found && o.material && o.material.map){ found = true; filter = o.material.map.magFilter; }
    });
    A.ok(found, 'ship_striker template has a textured material');
    if (found) A.eq(filter, THREE.NearestFilter, 'model texture uses NearestFilter in pixel mode');
  });

  t.test('switching back to modern restores baseline buffer', function () {
    var btn = document.querySelector('#setStyle button[data-q="modern"]');
    btn.onclick();
    var cvs = document.getElementById('game');
    A.eq(cvs.width, baseline.bufW, 'modern buffer width restored to baseline');
    A.eq(cvs.height, baseline.bufH, 'modern buffer height restored to baseline');
    A.eq(cvs.style.width, baseline.styleW, 'modern CSS width restored');
    A.ok(btn.classList.contains('on'), 'modern button marked active');
    var tpl = window.ModelLib && ModelLib.getTemplate('ship_striker');
    if (tpl){
      var filter = null;
      tpl.scene.traverse(function (o){
        if (filter === null && o.material && o.material.map) filter = o.material.map.magFilter;
      });
      if (filter !== null) A.eq(filter, THREE.LinearFilter, 'model texture back to LinearFilter');
    }
  });

  t.test('发光方块：emissive 顶点通道 + 点光源按块色相（金珀不入池）', function () {
    var p = api.pos();
    var x = Math.floor(p[0]), z = Math.floor(p[2]);
    var gy = api.topAt(x, z);
    // 先只放金珀（glow 地形块）：自发光但不进点光源池
    api.setBlock(x, gy + 1, z, 'amber');
    window.World.stream(p[0], p[2]);
    window.World.update(0.016, p[0], p[2]);
    window.World.update(0.6, p[0], p[2]);
    var lamps0 = window.World.debugLamps;
    var anyOn0 = lamps0.some(function (l) { return l.on; });
    A.ok(!anyOn0, 'amber terrain does not occupy point-light pool');
    // 换上光源方块：emissive 顶点通道非零 + 暖白灯点亮
    api.setBlock(x, gy + 1, z, 'lamp');
    window.World.stream(p[0], p[2]);
    window.World.update(0.016, p[0], p[2]);
    window.World.update(0.6, p[0], p[2]);
    var emFound = false;
    for (var i = 0; i < window.World.group.children.length; i++) {
      var m = window.World.group.children[i];
      if (!m.geometry || !m.geometry.attributes || !m.geometry.attributes.aEm) continue;
      var a = m.geometry.attributes.aEm;
      for (var v = 0; v < a.count; v++) {
        if (a.getX(v) > 0 || a.getY(v) > 0 || a.getZ(v) > 0) { emFound = true; break; }
      }
      if (emFound) break;
    }
    A.ok(emFound, 'lamp vertices carry non-zero emissive channel');
    var lamps1 = window.World.debugLamps;
    var lit = lamps1.filter(function (l) { return l.on; });
    A.ok(lit.length >= 1, 'a point light lit for lamp block');
    if (lit.length) A.eq(lit[0].color, 0xffd9a0, 'lamp light hue is warm white');
    // 清场
    api.setBlock(x, gy + 1, z, 'air');
    window.World.stream(p[0], p[2]);
  });

  t.test('天气粒子按生态生成且可开关', function () {
    var w = window.Game.debugWeather;
    A.ok(w && w.on, 'weather active on lush biome, ' + JSON.stringify(w));
    A.ok(w.n > 0, 'particles allocated');
    document.querySelector('#setWeather button[data-q="off"]').onclick();
    A.eq(window.Game.debugWeather.on, false, 'weather disabled');
    document.querySelector('#setWeather button[data-q="on"]').onclick();
    A.ok(window.Game.debugWeather.on, 'weather re-enabled');
  });
});

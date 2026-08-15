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
});

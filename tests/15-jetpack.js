/* STARFORGE 测试套件 15 — 喷气背包物理（净推力 > 重力：可爬升/制动坠落）
   确定性驱动：打开暂停面板冻结游戏循环（面板打开时主循环以 dt=0 更新玩家），
   再同步步进 Player.update(1/60) 精确积分，不依赖墙钟/帧率。 */
__SF_TEST__.suite('jetpack', function (t, api) {
  var A = api.assert;
  var CAM = {
    position: new THREE.Vector3(),
    quaternion: new THREE.Quaternion(),
    getWorldDirection: function (v) { v.set(0, 0, -1); return v; },
    updateMatrixWorld: function () {},
    add: function () {}, remove: function () {},
  };
  function step(n) { for (var i = 0; i < n; i++) window.Player.update(1 / 60, CAM); }

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('jetpack decelerates a free fall', function () {
    document.getElementById('pausePanel').classList.remove('hidden');   // 冻结游戏循环
    var p = api.pos();
    var gy = window.World.topAt(Math.floor(p[0]), Math.floor(p[2]));
    api.setPos(p[0], gy + 24, p[2]);
    api.setStat('jet', 100);
    window.Player.keys['Space'] = false;
    step(30);   // 自由落体 0.5s：v0 = -11 m/s
    var v0 = window.Player.vel.y;
    A.ok(v0 < -8, 'free fall gains downward speed (v0=' + v0.toFixed(2) + ')');
    window.Player.keys['Space'] = true;
    step(48);   // 喷气 0.8s：净推力 +11 m/s² → +8.8 m/s
    var v1 = window.Player.vel.y;
    window.Player.keys['Space'] = false;
    document.getElementById('pausePanel').classList.add('hidden');
    A.ok(v1 > v0 + 3, 'jet adds upward delta (v0=' + v0.toFixed(2) + ', v1=' + v1.toFixed(2) + ')');
    A.ok(v1 > -6, 'fall mostly arrested (v1=' + v1.toFixed(2) + ')');
  });

  t.test('jetpack consumes fuel while thrusting', function () {
    document.getElementById('pausePanel').classList.remove('hidden');
    var p = api.pos();
    var gy = window.World.topAt(Math.floor(p[0]), Math.floor(p[2]));
    api.setPos(p[0], gy + 24, p[2]);
    api.setStat('jet', 100);
    window.Player.keys['Space'] = true;
    step(36);   // 0.6s × 28/s = 16.8 燃料
    window.Player.keys['Space'] = false;
    document.getElementById('pausePanel').classList.add('hidden');
    var j = api.stats().jet;
    A.ok(j < 90 && j > 70, 'jet fuel drained at expected rate (jet=' + j.toFixed(1) + ')');
  });
});

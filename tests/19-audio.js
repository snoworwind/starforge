/* STARFORGE 测试套件 19 — 音频：持续音效生命周期（死亡/暂停停止，避免卡音） */
__SF_TEST__.suite('audio', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  function audioReady(){
    const c = Sound.ctx;
    return !!(c && c.state === 'running');
  }

  // 回归：Player.die() 置 dead 后 update 立即早退，喷气/激光的正常停止路径不可达——
  // 修复前音效穿过 1.8s 死亡淡出并持续到重生之后
  t.test('玩家死亡停止喷气与激光持续音', function () {
    Sound.begin();
    Sound.resume();
    if (audioReady()){
      // 真实音频路径：循环真的起振后再死，断言被停止
      Sound.loops.jet.start();
      Sound.loops.laser.start();
      A.ok(Sound.loops.jet.active && Sound.loops.laser.active, 'loops running before death');
      window.Player.damage(100);   // hp 归零触发 die()
      A.ok(!Sound.loops.jet.active && !Sound.loops.laser.active, 'loops stopped on death');
      return;
    }
    // 无可用音频设备（无头 CI）：间谍桩验证 die() 确实调用两个 loop 的 stop
    var calls = [];
    var j0 = Sound.loops.jet.stop, l0 = Sound.loops.laser.stop;
    Sound.loops.jet.stop = function(){ calls.push('jet'); };
    Sound.loops.laser.stop = function(){ calls.push('laser'); };
    try {
      window.Player.damage(100);   // hp 归零触发 die()
    } finally {
      Sound.loops.jet.stop = j0;
      Sound.loops.laser.stop = l0;
    }
    A.eq(calls.join(','), 'jet,laser', 'die() stops jet and laser loops (spy)');
  });

  // 回归：打开系统面板（ESC 菜单/设置/帮助/存档）时 worldPaused 冻结整个世界，
  // 但任何持续音都不停——激光/喷气/引擎/脉冲与氛围音乐在菜单后面继续响
  t.test('打开系统面板冻结世界时停止所有持续音与音乐', async function () {
    var names = ['engine', 'jet', 'laser', 'pulse', 'warp'];
    var stops = [];
    var originals = {};
    names.forEach(function (n){ originals[n] = Sound.loops[n].stop; });
    names.forEach(function (n){ Sound.loops[n].stop = function(){ stops.push(n); }; });
    var m0 = Sound.Music.stop;
    var musicStopped = false;
    Sound.Music.stop = function(){ musicStopped = true; };
    try {
      window.UI.toggle('pausePanel');   // 打开：下一帧 loop() 触发暂停边沿
      await api.waitUntil(function () { return stops.length === 5; }, 3000, 50);
      A.eq(stops.length, 5, 'all five loops stopped on pause edge (' + stops.join(',') + ')');
      A.ok(musicStopped, 'ambient music stopped on pause edge');
    } finally {
      names.forEach(function (n){ Sound.loops[n].stop = originals[n]; });
      Sound.Music.stop = m0;
      window.UI.closeAll();   // 关闭面板恢复世界运行（并触发恢复边沿）
    }
  });
});

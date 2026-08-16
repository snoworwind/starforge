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
});

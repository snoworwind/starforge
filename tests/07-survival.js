/* STARFORGE 测试套件 07 — 生存系统（伤害/护盾/充能/创造免疫） */
__SF_TEST__.suite('survival', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('damage drains shield before hp', function () {
    api.setStat('shield', 2); api.setStat('hp', 8);
    api.damage(1);
    A.eq(api.stats().shield, 1, 'shield 2->1');
    A.eq(api.stats().hp, 8, 'hp untouched');
  });

  t.test('damage over shield hits hp', function () {
    api.setStat('shield', 1); api.setStat('hp', 8);
    api.damage(2);
    A.eq(api.stats().shield, 0, 'shield empty');
    A.eq(api.stats().hp, 7, 'hp 8->7');
  });

  t.test('recharge consumes sodium and restores hazard', function () {
    api.setStat('haz', 10);
    api.give('sodium', 3);
    var before = api.count('sodium');
    A.ok(api.recharge('haz'), 'recharge ok');
    A.eq(api.count('sodium'), before - 1, 'sodium consumed');
    A.ok(api.stats().haz > 10, 'hazard restored');
  });

  t.test('chargeStat laser with carbon', function () {
    api.setStat('laser', 10);
    api.give('carbon', 5);
    var before = api.count('carbon');
    A.ok(api.chargeStat('laser'), 'charge ok');
    A.eq(api.count('carbon'), before - 3, 'carbon cost 3');
    A.ok(api.stats().laser > 10, 'laser restored');
  });

  t.test('canCharge reflects cost and fullness', function () {
    api.setStat('o2', 100);
    A.eq(api.canCharge('o2'), false, 'full, cannot charge');
    api.setStat('o2', 10);
    api.clearInv();
    A.eq(api.canCharge('o2'), false, 'no oxygen, cannot charge');
    api.give('oxygen', 1);
    A.ok(api.canCharge('o2'), 'can charge now');
  });

  t.test('creative mode immune to damage', function () {
    return api.boot('creative', { fresh: true }).then(function () {
      api.setStat('hp', 8); api.setStat('shield', 6);
      api.damage(5);
      A.eq(api.stats().hp, 8, 'hp unchanged in creative');
      A.eq(api.stats().shield, 6, 'shield unchanged in creative');
    });
  });

  // 回归：die() 重生只回满 hp/shield/o2/haz，漏掉喷气/激光——空槽死亡后重生仍是空槽
  t.test('死亡重生回满喷气与激光能量', async function () {
    return api.boot('normal', { fresh: true }).then(async function () {
      api.setStat('jet', 0);
      api.setStat('laser', 0);
      window.Player.damage(100);   // hp 归零触发 die()
      await api.sleep(2000);       // 1.8s 重生延迟
      var s = api.stats();
      A.ok(s.jet >= s.jetMax - 0.01 && s.laser >= s.laserMax - 0.01,
        'jet/laser restored on respawn (jet=' + s.jet + '/' + s.jetMax + ' laser=' + s.laser + '/' + s.laserMax + ')');
    });
  });

  // 回归：deserialize 直接读 d.inv[i]——旧档/脏数据缺 inv 字段时抛 TypeError，整个读档中断
  t.test('deserialize tolerates missing inv field (empty backpack fallback)', function () {
    api.give('carbon', 5);
    // 缺 inv 的脏数据：修复前抛 TypeError（读档整体失败），修复后按空背包恢复其余字段
    window.Player.deserialize({ pos: [0, 40, 0], yaw: 0, pitch: 0, stats: { hp: 12, hpMax: 20 }, hotIdx: 0, credits: 77, appearance: null });
    var s = api.stats();
    A.eq(s.hp, 12, 'stats restored despite missing inv');
    A.eq(api.credits(), 77, 'credits restored despite missing inv');
    A.eq(api.inv().filter(Boolean).length, 0, 'inventory treated as empty');
  });

  // 回归：damageTick 每次触发后 dmgAcc 归零——跨过 1.0 的余数被吞，慢速持续伤害的实际速率低于标称
  t.test('damageTick keeps fractional remainder (slow DoT accumulates)', function () {
    api.setStat('shield', 0);
    api.setStat('hp', 40);
    for (var i = 0; i < 10; i++) window.Player.debugDamageTick(1, 0.75);   // 0.75/s × 10s = 7.5 → 7 点
    A.eq(api.stats().hp, 33, 'accumulated 7 damage (修复前 5：余数每 tick 被归零, got ' + api.stats().hp + ')');
  });

  // 坠落伤害契约：阈值与公式基准一致（-12）——v=-16 恰 1 点、v=-13 安全无伤。
  // 修复本身是阈值/公式一致性（无行为变化），本用例锁定伤害曲线防未来漂移
  t.test('fall damage contract: v=-16 deals 1, v=-13 deals 0', function () {
    function dropWith(v){
      var p = api.pos();
      var gy = api.topAt(Math.floor(p[0]), Math.floor(p[2]));
      api.setStat('hp', 20); api.setStat('shield', 0);
      api.setPos(p[0], gy + 1.2, p[2]);
      window.Player.vel.y = v;
      var cam = new THREE.PerspectiveCamera(75, 1, 0.1, 100);
      cam.position.set(99999, 99999, 99999);   // 远离场景：避免射线/幽灵预览干扰
      window.Player.update(1 / 60, cam);   // 单帧受控坠落（重力后撞击速度 ≈ v - 0.33）
      return api.stats().hp;
    }
    var hp1 = dropWith(-16);
    A.eq(hp1, 19, 'v=-16 impact deals exactly 1 (got hp ' + hp1 + ')');
    var hp2 = dropWith(-13);
    A.eq(hp2, 20, 'v=-13 impact deals 0 (got hp ' + hp2 + ')');
  });
});

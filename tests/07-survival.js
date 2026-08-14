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
});

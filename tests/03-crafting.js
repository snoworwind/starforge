/* STARFORGE 测试套件 03 — 便携合成（dropMult=1 逻辑 + 生存难度倍率） */
__SF_TEST__.suite('crafting', function (t, api) {
  var A = api.assert;

  t.before(function () { api.clearInv(); api.setCredits(0); });

  t.test('craft planks consumes and produces (dropMult 1)', function () {
    api.clearInv();
    api.give('carbon', 4);
    A.eq(api.craft('planks_b', 1), 1, 'made 1 batch');
    A.eq(api.count('carbon'), 0, 'carbon consumed');
    A.eq(api.count('planks_b'), 4, '4 planks produced');
  });

  t.test('craft requires materials', function () {
    api.clearInv();
    A.eq(api.craft('gear', 1), 0, 'no gear without iron');
    A.eq(api.count('gear'), 0);
  });

  t.test('craft returns made count and stops when out', function () {
    api.clearInv();
    api.give('iron', 20);
    api.give('carbon', 20);
    // plate: iron×3 + carbon×2 → plate×1
    A.eq(api.craft('plate', 10), 6, 'only 6 plates (iron 20/3 floor)');
    A.eq(api.count('plate'), 6);
  });

  t.test('tech-gated recipe blocked before research', function () {
    api.clearInv();
    api.give('iron', 8); api.give('gear', 4); api.give('stone', 6);
    A.eq(api.canCraft('burner_b'), false, 'burner_b gated by automation');
    A.eq(api.craft('burner_b', 1), 0, 'cannot craft gated recipe');
    A.eq(api.count('burner_b'), 0);
  });

  t.test('fuel recipe composition', function () {
    api.clearInv();
    api.give('carbon', 25); api.give('oxygen', 10);
    A.eq(api.craft('fuel', 1), 1, 'fuel crafted');
    A.eq(api.count('fuel'), 1);
  });

  t.test('survival dropMult multiplies output', function () {
    return api.boot('normal', { fresh: true }).then(function () {
      api.clearInv();
      api.give('carbon', 4);
      A.eq(api.craft('planks_b', 1), 1);
      A.eq(api.count('planks_b'), 4 * api.snapshot().dropMult, 'output × dropMult');
    });
  });
});

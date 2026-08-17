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

  t.test('craft with full backpack drops overflow instead of vanishing', function () {
    // 需要星球场景（dropGroup 挂在世界场景上）才能真实掉落
    return api.boot('normal', { fresh: true }).then(function () {
      api.clearInv();
      // 36 格全部填满，第 0 格放原料
      for (var i = 0; i < 36; i++) window.Player.inv[i] = { item: 'stone', n: 250 };
      window.Player.inv[0] = { item: 'carbon', n: 250 };
      var d0 = window.Player.dropCount;
      var r = RECIPE_BY_ID['planks_b'];
      A.ok(r, 'recipe lookup available');
      A.eq(window.UI.tryCraft(r), true, 'craft completes even when backpack full');
      A.eq(api.count('carbon'), 246, '4 carbon consumed');
      A.eq(api.count('planks_b'), 0, 'no room in backpack');
      A.eq(window.Player.dropCount, d0 + 1, 'overflow planks dropped beside player');
    });
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

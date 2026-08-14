/* STARFORGE 测试套件 02 — 背包系统（无需启动星球） */
__SF_TEST__.suite('inventory', function (t, api) {
  var A = api.assert;

  t.before(function () { api.clearInv(); api.setCredits(0); });

  t.test('add and count', function () {
    A.eq(api.give('carbon', 10), 10, 'added 10');
    A.eq(api.count('carbon'), 10, 'count 10');
  });

  t.test('per-slot stack cap (250), overflow to new slot', function () {
    api.clearInv();
    api.give('carbon', 200);
    api.give('carbon', 100);
    A.eq(api.count('carbon'), 300, 'total 300 across two slots');
    var stacks = api.inv().filter(Boolean).filter(function (s) { return s.item === 'carbon'; });
    A.ok(stacks.every(function (s) { return s.n <= 250; }), 'no slot exceeds 250');
    A.ok(stacks.length >= 2, 'spilled into a second slot');
  });

  t.test('addItem returns actual added (no silent loss)', function () {
    api.clearInv();
    A.eq(api.give('carbon', 300), 300, 'all 300 added (250 + 50)');
  });

  t.test('removeItem subtracts and reports success', function () {
    api.clearInv();
    api.give('carbon', 10);
    A.ok(api.take('carbon', 4), 'remove 4');
    A.eq(api.count('carbon'), 6, '6 left');
    A.ok(!api.take('carbon', 99), 'cannot remove 99');
    A.eq(api.count('carbon'), 6, 'still 6 after failed remove');
  });

  t.test('has / count zero', function () {
    api.clearInv();
    A.ok(!api.has('iron'), 'no iron');
    api.give('iron', 3);
    A.ok(api.has('iron', 3), 'has 3');
    A.ok(!api.has('iron', 4), 'not 4');
  });

  t.test('clearInv empties', function () {
    api.give('sodium', 5);
    api.give('oxygen', 3);
    api.clearInv();
    A.eq(api.count('sodium'), 0);
    A.eq(api.inv().filter(Boolean).length, 0, 'all 36 slots empty');
  });

  t.test('multiple stacks counted together', function () {
    api.clearInv();
    api.give('stone', 250);
    api.give('stone', 250);
    api.give('stone', 50);
    A.eq(api.count('stone'), 550, 'count across 3 stacks');
  });
});

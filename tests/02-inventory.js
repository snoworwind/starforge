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

  t.test('sortInventory merges storage stacks and compacts, hotbar untouched', function () {
    api.clearInv();
    // 快捷栏排布：格子 0 = 铁，格子 2 = 碳（整理后必须原样）
    window.Player.inv[0] = { item: 'iron', n: 5 };
    window.Player.inv[2] = { item: 'carbon', n: 7 };
    // 储存舱：分散的同类堆叠 + 空位（9=碳200, 12=碳80, 13=铁3, 17=土4）
    window.Player.inv[9]  = { item: 'carbon', n: 200 };
    window.Player.inv[12] = { item: 'carbon', n: 80 };
    window.Player.inv[13] = { item: 'iron', n: 3 };
    window.Player.inv[17] = { item: 'dirt', n: 4 };
    A.ok(window.Player.sortInventory(), 'sort ok');
    // 快捷栏保持
    A.eq(window.Player.inv[0].item, 'iron', 'hotbar slot 0 untouched');
    A.eq(window.Player.inv[0].n, 5, 'hotbar slot 0 count untouched');
    A.eq(window.Player.inv[2].item, 'carbon', 'hotbar slot 2 untouched');
    A.eq(window.Player.inv[2].n, 7, 'hotbar slot 2 count untouched');
    // 储存舱合并：碳 280 → 250 + 30 两格；铁 3 一格；土 4 一格；其余空
    A.eq(window.Player.inv[9].item, 'carbon', 'first storage stack carbon');
    A.eq(window.Player.inv[9].n, 250, 'capped at 250');
    A.eq(window.Player.inv[10].item, 'carbon', 'spill stack carbon');
    A.eq(window.Player.inv[10].n, 30, 'spill 30');
    A.eq(window.Player.inv[11].item, 'iron', 'iron consolidated');
    A.eq(window.Player.inv[11].n, 3, 'iron 3');
    A.eq(window.Player.inv[12].item, 'dirt', 'dirt moved up');
    A.eq(window.Player.inv[12].n, 4, 'dirt 4');
    A.ok(window.Player.inv.slice(13).every(function (s) { return s === null; }), 'rest compacted empty');
    api.clearInv();
  });
});

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

  // 回归：货仓「热栏选中物整组存入」用 Player.removeItem 扣物——removeItem 从最高索引开始扣，
  // 若储存舱有同类物品，被清空的是别的堆（选中格原封不动），语义与提示完全相反
  t.test('ship cargo deposit moves the selected hotbar stack (not a storage stack)', function () {
    api.clearInv();
    window.UI.closeAll();
    window.Player.inv[0] = { item: 'iron', n: 5 };    // 热栏选中格
    window.Player.inv[20] = { item: 'iron', n: 3 };   // 储存格同物品（removeItem 会先扣它）
    window.Player.hotIdx = 0;
    window.UI.toggle('invPanel');
    var shipBtn = document.querySelector('#invTabs .invtab[data-t="ship"]');
    A.ok(shipBtn, 'ship tab built');
    shipBtn.onclick();
    var slots = document.querySelectorAll('#shipInvGrid [data-ssi]');
    A.ok(slots.length > 0, 'cargo grid built');
    slots[0].onclick();   // 空手点空格 → 热栏选中物整组存入
    A.eq(window.Player.inv[0], null, 'selected hotbar slot emptied');
    A.ok(window.Player.inv[20] && window.Player.inv[20].item === 'iron' && window.Player.inv[20].n === 3,
      'untouched storage stack intact (got ' + JSON.stringify(window.Player.inv[20]) + ')');
    var cnt0 = document.querySelector('#shipInvGrid [data-ssi="0"] .cnt');
    A.eq(cnt0 && cnt0.textContent, '5', 'cargo slot 0 received the full stack ×5');
    // 清理：空手再点 0 号格取回背包，随后清空背包——不留货仓/背包残留污染后续套件
    document.querySelector('#shipInvGrid [data-ssi="0"]').onclick();
    api.clearInv();
    window.UI.closeAll();
  });

  // 回归：掉落物上限（90）回收最旧掉落时忽略 addItem 返回值——背包满时被顶掉的掉落凭空消失
  t.test('drop cap recycling keeps overflow (no item loss)', function () {
    // 不 boot（boot 会改难度倍率，污染后续合成套件的 dropMult=1 假设）：
    // 用一次性场景初始化掉落容器，直接驱动真实 spawnDrop 路径
    window.Player.initVisuals(new THREE.Scene());
    api.clearInv();
    for (var i = 0; i < 36; i++) window.Player.inv[i] = { item: 'stone', n: 250 };   // 背包全满
    // 90 个铁分散放置（避免同点合并），再放 1 个触发上限回收
    for (var j = 0; j < 90; j++) window.Player.spawnDrop((j % 10) * 2, 45, Math.floor(j / 10) * 2, 'iron', 1);
    A.eq(window.Player.dropCount, 90, '90 drops on ground');
    window.Player.spawnDrop(30, 45, 30, 'iron', 1);
    var items = window.Player.debugDropItems();
    var iron = 0;
    for (var k = 0; k < items.length; k++) if (items[k].item === 'iron') iron += items[k].n;
    A.eq(iron, 91, 'no iron lost through cap recycling (修复前 90, got ' + iron + ')');
    api.clearInv();
  });
});

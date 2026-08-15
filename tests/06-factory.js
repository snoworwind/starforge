/* STARFORGE 测试套件 06 — 工厂自动化（放置/拆除/冶炼/采矿/装配/精炼/电力/物流） */
__SF_TEST__.suite('factory', function (t, api) {
  var A = api.assert;
  var X = 40, Z = 40;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  function groundY() { return api.topAt(X, Z); }
  function insertN(x, y, z, item, n) {
    var c = 0;
    for (var i = 0; i < n; i++) if (api.machineInsert(x, y, z, item)) c++;
    return c;
  }

  t.test('place / inspect / remove machine', function () {
    var y = groundY() + 1;
    api.placeMachine('chest', X, y, Z, 0);
    A.ok(api.machineAt(X, y, Z), 'machine placed');
    A.eq(api.machineAt(X, y, Z).type, 'chest');
    A.eq(api.blockKeyAt(X, y, Z), 'chest', 'block set to chest');
    api.removeMachine(X, y, Z);
    A.eq(api.machineAt(X, y, Z), null, 'machine removed');
    A.eq(api.blockKeyAt(X, y, Z), 'air', 'block back to air');
  });

  t.test('chest stacks same item, holds 24 distinct slots', function () {
    var y = groundY() + 1;
    // 同类物品合并进同一格
    api.placeMachine('chest', X, y, Z, 0);
    A.eq(insertN(X, y, Z, 'stone', 24), 24, '24 inserted');
    var m = api.machineAt(X, y, Z);
    A.eq(m.data.slots[0].n, 24, 'stone merged into one stack');
    A.ok(m.data.slots.slice(1).every(function (s) { return s === null; }), 'other slots empty');
    api.removeMachine(X, y, Z);

    // 24 个不同物品占满 24 格
    var ids = ['carbon','oxygen','sodium','coal','iron_ore','copper_ore','titanium_ore','gold_ore','uranium','tritium','iron','copper','titanium','gold','gear','wire','circuit','plate','data','fuel','glass_b','planks_b','stone','dirt'];
    api.placeMachine('chest', X, y, Z, 0);
    ids.forEach(function (id) { api.machineInsert(X, y, Z, id); });
    var m2 = api.machineAt(X, y, Z);
    A.eq(m2.data.slots.filter(Boolean).length, 24, '24 distinct slots filled');
    A.eq(api.machineInsert(X, y, Z, 'sand'), false, 'chest full: 25th distinct item rejected');
    api.removeMachine(X, y, Z);
  });

  t.test('furnace smelts iron ore into iron', function () {
    var y = groundY() + 1;
    api.placeMachine('furnace', X, y, Z, 0);
    api.machineInsert(X, y, Z, 'iron_ore');
    api.machineInsert(X, y, Z, 'coal');
    api.tickFactory(4.0, 1);
    var m = api.machineAt(X, y, Z);
    A.ok(m.data.out && m.data.out.item === 'iron', 'iron smelted, out=' + JSON.stringify(m.data.out));
    api.removeMachine(X, y, Z);
  });

  t.test('miner extracts ore below', function () {
    var y = groundY() + 1;
    api.setBlock(X, y - 1, Z, 'iron_ore');
    api.placeMachine('miner', X, y, Z, 0);
    api.tickFactory(10.0, 1);
    var m = api.machineAt(X, y, Z);
    A.ok(m.data.out && m.data.out.item === 'iron_ore', 'miner output iron_ore, out=' + JSON.stringify(m.data.out));
    A.ok(m.data.deposit > 0, 'deposit incremented');
    api.removeMachine(X, y, Z);
    api.setBlock(X, y - 1, Z, 'air');
  });

  t.test('assembler crafts gear with solar power', function () {
    var y = groundY() + 1;
    api.placeMachine('solar', X, y, Z + 2, 0);
    api.placeMachine('assembler', X, y, Z, 0);
    api.setMachineRecipe(X, y, Z, 'gear');
    api.machineInsert(X, y, Z, 'iron');
    api.machineInsert(X, y, Z, 'iron');
    api.tickFactory(3.0, 1);
    var m = api.machineAt(X, y, Z);
    A.ok(m.data.out && m.data.out.item === 'gear', 'gear assembled, out=' + JSON.stringify(m.data.out));
    A.ok(api.power().gen >= 10, 'solar gen >= 10');
    api.removeMachine(X, y, Z);
    api.removeMachine(X, y, Z + 2);
  });

  t.test('refinery crafts fuel with reactor power', function () {
    var y = groundY() + 1;
    api.placeMachine('reactor', X, y, Z + 2, 0);
    api.machineInsert(X, y, Z + 2, 'uranium');
    api.placeMachine('refinery', X, y, Z, 0);
    api.setMachineRecipe(X, y, Z, 'fuel');
    A.eq(insertN(X, y, Z, 'carbon', 25), 25, 'carbon in');
    A.eq(insertN(X, y, Z, 'oxygen', 10), 10, 'oxygen in');
    api.tickFactory(10.0, 1);
    var m = api.machineAt(X, y, Z);
    A.ok(m.data.out && m.data.out.item === 'fuel', 'fuel refined, out=' + JSON.stringify(m.data.out));
    api.removeMachine(X, y, Z);
    api.removeMachine(X, y, Z + 2);
  });

  t.test('belt transports item into chest', function () {
    var y = groundY() + 1;
    api.placeMachine('belt', X, y, Z, 0);       // dir 0 = +x
    api.placeMachine('chest', X + 1, y, Z, 0);
    api.machineInsert(X, y, Z, 'stone');
    api.tickFactory(2.0, 1);
    var chest = api.machineAt(X + 1, y, Z);
    A.ok(chest.data.slots.some(function (s) { return s && s.item === 'stone'; }), 'stone reached chest');
    api.removeMachine(X, y, Z);
    api.removeMachine(X + 1, y, Z);
  });

  t.test('power: solar/burner/reactor/wind', function () {
    var y = groundY() + 1;
    api.placeMachine('solar', X, y, Z, 0);
    api.tickFactory(1.0, 1);
    A.eq(api.power().gen, 10, 'solar 10kW');
    api.removeMachine(X, y, Z);

    api.placeMachine('burner', X, y, Z, 0);
    api.machineInsert(X, y, Z, 'coal');
    api.tickFactory(1.0, 1);
    A.eq(api.power().gen, 25, 'burner 25kW');
    api.removeMachine(X, y, Z);

    api.placeMachine('reactor', X, y, Z, 0);
    api.machineInsert(X, y, Z, 'uranium');
    api.tickFactory(1.0, 1);
    A.eq(api.power().gen, 100, 'reactor 100kW');
    api.removeMachine(X, y, Z);

    api.placeMachine('wind', X, y, Z, 0);
    api.tickFactory(1.0, 1);
    A.between(api.power().gen, 2, 16, 'wind 2..16kW');
    api.removeMachine(X, y, Z);
  });

  t.test('collector stores 12 slots', function () {
    var y = groundY() + 1;
    api.placeMachine('collector', X, y, Z, 0);
    A.eq(insertN(X, y, Z, 'carbon', 12), 12, '12 carbon inserted');
    api.removeMachine(X, y, Z);
  });

  t.test('beacon stores label', function () {
    var y = groundY() + 1;
    api.placeMachine('beacon', X, y, Z, 0);
    A.eq(api.machineAt(X, y, Z).data.label, '标记点', 'default beacon label');
    api.removeMachine(X, y, Z);
  });

  t.test('launchpad places cleanly', function () {
    var y = groundY() + 1;
    api.placeMachine('launchpad', X, y, Z, 0);
    A.ok(api.machineAt(X, y, Z), 'launchpad placed');
    api.removeMachine(X, y, Z);
  });

  t.test('lumberbot spawns bot on first tick', function () {
    var y = groundY() + 1;
    api.placeMachine('lumberbot', X, y, Z, 0);
    api.tickFactory(1.0, 1);
    var m = api.machineAt(X, y, Z);
    A.eq(m.data.cargo, 0, 'starts empty');
    A.ok(window.Factory.at(X, y, Z).bot, 'bot entity created');
    api.removeMachine(X, y, Z);
  });

  t.test('factory serialize/deserialize roundtrip', function () {
    var y = groundY() + 1;
    api.placeMachine('chest', X, y, Z, 0);
    api.machineInsert(X, y, Z, 'iron');
    var s = window.Factory.serialize();
    api.removeMachine(X, y, Z);
    window.Factory.deserialize(s);
    var m = api.machineAt(X, y, Z);
    A.ok(m && m.type === 'chest', 'chest restored');
    A.ok(m.data.slots.some(function (sl) { return sl && sl.item === 'iron'; }), 'inventory restored');
    window.Factory.reset();
    api.setBlock(X, y, Z, 'air');
  });

  t.test('power: 空闲机器不计入电网需求', function () {
    var y = groundY() + 1;
    // 1 块太阳能（10kW）+ 2 台空闲装配机（无配方）：不应产生需求，sat=1
    api.placeMachine('solar', X, y, Z, 0);
    api.placeMachine('assembler', X + 1, y, Z, 0);
    api.placeMachine('assembler', X + 2, y, Z, 0);
    api.tickFactory(1.0, 1);
    A.eq(api.power().use, 0, 'idle assemblers draw no power');
    A.eq(api.power().sat, 1, 'sat full with idle machines');
    // 给其中一台设置配方+原料（6 铁可撑多 tick 消耗）：只有它计入 12kW 需求
    api.setMachineRecipe(X + 1, y, Z, 'gear');
    var i;
    for (i = 0; i < 6; i++) api.machineInsert(X + 1, y, Z, 'iron');
    api.tickFactory(1.0, 1);
    A.eq(api.power().use, 12, 'only working assembler counts');
    A.ok(api.power().sat < 1, 'sat drops below 1 (10kW < 12kW)');
    api.removeMachine(X, y, Z);
    api.removeMachine(X + 1, y, Z);
    api.removeMachine(X + 2, y, Z);
  });

  t.test('power: 无矿采矿机不计入电网需求', function () {
    var y = groundY() + 1;
    api.placeMachine('solar', X, y, Z + 2, 0);
    api.placeMachine('miner', X, y, Z, 0);   // 脚下是普通地面，无矿脉
    api.tickFactory(1.0, 1);
    A.eq(api.power().use, 0, 'miner without ore below draws no power');
    A.eq(api.power().sat, 1, 'sat full');
    // 放上矿脉后开始计费
    api.setBlock(X, y - 1, Z, 'iron_ore');
    api.tickFactory(1.0, 1);
    A.eq(api.power().use, 8, 'miner over ore draws 8kW');
    api.removeMachine(X, y, Z);
    api.removeMachine(X, y, Z + 2);
    api.setBlock(X, y - 1, Z, 'air');
  });

  t.test('furnace: 原料不足不点火不空烧', function () {
    var y = groundY() + 1;
    api.placeMachine('furnace', X, y, Z, 0);
    api.machineInsert(X, y, Z, 'sand');   // 1 沙 < 需要 2（烧玻璃），不足一份
    api.machineInsert(X, y, Z, 'coal');
    api.tickFactory(4.0, 1);
    var m = api.machineAt(X, y, Z);
    A.eq(m.data.burn, 0, 'insufficient input: burner never lit');
    A.eq(m.data.fuel.n, 1, 'fuel not wasted');
    A.eq(m.data.out, null, 'no output');
    // 补足到 2：正常点火冶炼
    api.machineInsert(X, y, Z, 'sand');
    api.tickFactory(4.0, 1);
    m = api.machineAt(X, y, Z);
    A.ok(m.data.out && m.data.out.item === 'glass_b', '2 sand smelts after refill, out=' + JSON.stringify(m.data.out));
    api.removeMachine(X, y, Z);
  });

  t.test('熔炉/采矿机可向侧面传送带输出（非装配机型不受输入面限制）', function () {
    var y = groundY() + 1;
    // 熔炉：正面 +x，侧面 -z 放传送带（dir 2 朝 -z 流走）
    api.placeMachine('furnace', X, y, Z, 0);
    api.placeMachine('belt', X, y, Z - 1, 2);
    api.machineInsert(X, y, Z, 'iron_ore');
    api.machineInsert(X, y, Z, 'coal');
    api.tickFactory(5.0, 1);
    var belt = api.machineAt(X, y, Z - 1);
    A.ok(belt.data.items.length > 0, 'furnace outputs to side belt, items=' + JSON.stringify(belt.data.items));
    api.removeMachine(X, y, Z);
    api.removeMachine(X, y, Z - 1);
    // 采矿机：矿脉在脚下，侧面放皮带（dir 1 = +z，流向远处）
    api.setBlock(X, y - 1, Z, 'iron_ore');
    api.placeMachine('miner', X, y, Z, 0);
    api.placeMachine('belt', X, y, Z + 1, 1);
    api.tickFactory(6.0, 1);
    var belt2 = api.machineAt(X, y, Z + 1);
    A.ok(belt2.data.items.length > 0, 'miner outputs to side belt, items=' + JSON.stringify(belt2.data.items));
    api.removeMachine(X, y, Z);
    api.removeMachine(X, y, Z + 1);
    api.setBlock(X, y - 1, Z, 'air');
  });

  t.test('医疗站：站近消耗钠+氧气治疗，供电不足不开工', function () {
    var y = groundY() + 1;
    api.placeMachine('solar', X, y, Z + 2, 0);
    api.placeMachine('medbay', X, y, Z, 0);
    api.setPos(X + 0.5, y + 0.2, Z + 0.5);   // 站到医疗站旁
    api.setStat('shield', 0);
    api.setStat('hp', 3);                    // hpMax=8：缺 5 点
    api.clearInv();
    api.give('sodium', 10);
    api.give('oxygen', 10);
    api.tickFactory(3.0, 1);
    var hp = api.stats().hp;
    A.ok(hp > 3, 'healed (hp=' + hp + ')');
    A.ok(api.count('sodium') < 10, 'sodium consumed (na=' + api.count('sodium') + ')');
    A.ok(api.count('oxygen') < 10, 'oxygen consumed (ox=' + api.count('oxygen') + ')');
    api.removeMachine(X, y, Z);
    api.removeMachine(X, y, Z + 2);
    api.setStat('hp', 8);
  });

  t.test('医疗站：满血/缺补给时不产生电网需求', function () {
    var y = groundY() + 1;
    api.placeMachine('solar', X, y, Z + 2, 0);
    api.placeMachine('medbay', X, y, Z, 0);
    api.setPos(X + 0.5, y + 0.2, Z + 0.5);
    api.setStat('hp', 8);   // 满血
    api.clearInv();
    api.tickFactory(1.0, 1);
    A.eq(api.power().use, 0, 'no demand when full hp');
    api.setStat('hp', 3);    // 缺补给
    api.tickFactory(1.0, 1);
    A.eq(api.power().use, 0, 'no demand without supplies');
    api.give('sodium', 5); api.give('oxygen', 5);
    api.tickFactory(1.0, 1);
    A.eq(api.power().use, 6, 'demand 6kW when healing possible');
    api.removeMachine(X, y, Z);
    api.removeMachine(X, y, Z + 2);
    api.setStat('hp', 8);
  });

  t.test('power: 装配机开工扣料后仍计入电网需求（供电不足不得全速白跑）', function () {
    var y = groundY() + 1;
    api.placeMachine('solar', X, y, Z + 2, 0);       // 10kW
    api.placeMachine('assembler', X, y, Z, 0);       // 12kW
    api.setMachineRecipe(X, y, Z, 'gear');
    api.machineInsert(X, y, Z, 'iron');
    api.machineInsert(X, y, Z, 'iron');
    api.tickFactory(0.5, 1);   // 第 1 tick：原料足 → 计入 12kW 需求并扣料开工
    var m = api.machineAt(X, y, Z);
    A.ok(m.data.prog > 0, 'craft started (prog=' + m.data.prog + ')');
    A.eq(m.data.in.iron, 0, 'inputs deducted');
    api.tickFactory(1.0, 1);   // 第 2 tick：原料已扣，但制作中仍须计费
    A.eq(api.power().use, 12, 'mid-craft demand stays 12kW (got ' + api.power().use + ')');
    A.ok(api.power().sat < 1, 'sat throttled below 1 (10kW < 12kW, sat=' + api.power().sat + ')');
    api.removeMachine(X, y, Z);
    api.removeMachine(X, y, Z + 2);
  });

  t.test('miner: 矿脉耗尽后缓存矿石仍可送出（不再永久卡死）', function () {
    var y = groundY() + 1;
    api.setBlock(X, y - 1, Z, 'iron_ore');
    api.placeMachine('solar', X, y, Z + 2, 0);   // 满电全速：2 秒/矿
    api.placeMachine('miner', X, y, Z, 0);
    api.tickFactory(7.0, 1);   // 无输出邻居 → 矿石积压缓存
    var m = api.machineAt(X, y, Z);
    A.ok(m.data.out && m.data.out.n > 0, 'miner buffered ore (out=' + JSON.stringify(m.data.out) + ')');
    api.placeMachine('chest', X + 1, y, Z, 0);
    api.setBlock(X, y - 1, Z, 'stone');   // 矿脉耗尽（下方变岩石）
    api.tickFactory(4.0, 1);
    var chest = api.machineAt(X + 1, y, Z);
    A.ok(chest.data.slots.some(function (s) { return s && s.item === 'iron_ore'; }), 'buffered ore drained to chest (slots=' + JSON.stringify(chest.data.slots) + ')');
    api.removeMachine(X, y, Z);
    api.removeMachine(X, y, Z + 2);
    api.removeMachine(X + 1, y, Z);
    api.setBlock(X, y - 1, Z, 'air');
  });
});

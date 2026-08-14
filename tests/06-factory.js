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
});

/* STARFORGE 测试套件 08 — 任务线（完整推进 21 步 + 门槛校验 + 创造跳过） */
__SF_TEST__.suite('quests', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('full quest line advances to completion', function () {
    var Q = api.defs.QUESTS;
    A.eq(api.questIdx(), 0, 'start at quest 0');

    // 1 苏醒（检查飞船）
    api.setFlag('checkedShip', true); api.pokeQuests();
    A.eq(api.questId(), 'q_carbon');

    // 2 采集碳 ×15
    api.give('carbon', 15); api.pokeQuests();
    A.eq(api.questId(), 'q_sodium');

    // 3 采集钠 ×8
    api.give('sodium', 8); api.pokeQuests();
    A.eq(api.questId(), 'q_stone');

    // 4 采集岩石 ×12
    api.give('stone', 12); api.pokeQuests();
    A.eq(api.questId(), 'q_furnace');

    // 5 放置熔炉
    api.placeEvent('furnace');
    A.eq(api.questId(), 'q_iron');

    // 6 熔炼铁锭 ×10
    api.give('iron', 10); api.pokeQuests();
    A.eq(api.questId(), 'q_repair');

    // 7 修复推进器
    api.setFlag('shipRepaired', true); api.pokeQuests();
    A.eq(api.questId(), 'q_tech');

    // 8 研究冶金学
    api.give('data', 99);
    A.ok(api.research('metallurgy'), 'research metallurgy');
    A.eq(api.questId(), 'q_auto');

    // 9 放置采矿机
    api.placeEvent('miner');
    A.eq(api.questId(), 'q_belt');

    // 10 传送带 ×6
    for (var i = 0; i < 6; i++) api.placeEvent('belt');
    A.eq(api.questId(), 'q_power');

    // 11 太阳能板 ×2
    api.placeEvent('solar'); api.placeEvent('solar');
    A.eq(api.questId(), 'q_refinery');

    // 12 精炼厂
    api.placeEvent('refinery');
    A.eq(api.questId(), 'q_fuel');

    // 13 发射燃料 ×2
    api.give('fuel', 2); api.pokeQuests();
    A.eq(api.questId(), 'q_launch');

    // 14 起飞
    api.setFlag('launched', true); api.pokeQuests();
    A.eq(api.questId(), 'q_station');

    // 15 停靠空间站
    api.setFlag('docked', true); api.pokeQuests();
    A.eq(api.questId(), 'q_trade');

    // 16 交易
    api.setFlag('traded', true); api.pokeQuests();
    A.eq(api.questId(), 'q_explore');

    // 17 新世界
    api.setFlag('newPlanet', true); api.pokeQuests();
    A.eq(api.questId(), 'q_nuclear');

    // 18 核反应堆
    api.placeEvent('reactor');
    A.eq(api.questId(), 'q_antimatter');

    // 19 反物质 ×3
    api.give('antimatter', 3); api.pokeQuests();
    A.eq(api.questId(), 'q_warp');

    // 20 曲率电池
    api.give('warpcell', 1); api.pokeQuests();
    A.eq(api.questId(), 'q_leave');

    // 21 跃迁离开
    api.setFlag('warpedOut', true); api.pokeQuests();
    A.eq(api.questId(), null, 'quest line complete');
    A.eq(api.questIdx(), Q.length, 'questIdx == QUESTS.length');
  });

  t.test('collect quest requires full amount', function () {
    return api.boot('normal', { fresh: true }).then(function () {
      api.clearInv();   // 清除 newGame 赠送的起始物资（碳×10 钠×5）
      api.setFlag('checkedShip', true); api.pokeQuests();
      A.eq(api.questId(), 'q_carbon');
      api.give('carbon', 14); api.pokeQuests();
      A.eq(api.questId(), 'q_carbon', 'still q_carbon at 14/15');
      api.give('carbon', 1); api.pokeQuests();
      A.eq(api.questId(), 'q_sodium', 'advances at 15/15');
    });
  });

  t.test('creative mode skips quests', function () {
    return api.boot('creative', { fresh: true }).then(function () {
      A.eq(api.questIdx(), 0, 'questIdx stays 0 in creative');
      api.setFlag('checkedShip', true); api.pokeQuests();
      A.eq(api.questIdx(), 0, 'still 0 after poke in creative');
    });
  });
});

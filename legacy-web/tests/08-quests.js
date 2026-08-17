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

  // 回归：flags.newPlanet 一旦置位永不清除——提前探索第二颗星球的玩家到达 q_explore 时
  // 任务被瞬时完成（旗标提前满足）。修复：q_explore 激活时作废旧旗标，必须重新着陆
  t.test('q_explore 要求新着陆：提前着陆的旗标不重复结算', function () {
    return api.boot('normal', { fresh: true }).then(function () {
      window.Game.flags.newPlanet = true;   // 提前「探索过第二颗星球」
      api.setFlag('checkedShip', true); api.pokeQuests();
      api.give('carbon', 15); api.pokeQuests();
      api.give('sodium', 8); api.pokeQuests();
      api.give('stone', 12); api.pokeQuests();
      api.placeEvent('furnace');
      api.give('iron', 10); api.pokeQuests();
      api.setFlag('shipRepaired', true); api.pokeQuests();
      api.give('data', 99); api.research('metallurgy');
      api.placeEvent('miner');
      for (var i = 0; i < 6; i++) api.placeEvent('belt');
      api.placeEvent('solar'); api.placeEvent('solar');
      api.placeEvent('refinery');
      api.give('fuel', 2); api.pokeQuests();
      api.setFlag('launched', true); api.pokeQuests();
      api.setFlag('docked', true); api.pokeQuests();
      api.setFlag('traded', true); api.pokeQuests();
      A.eq(api.questId(), 'q_explore', 'reached q_explore');
      api.pokeQuests();
      A.eq(api.questId(), 'q_explore', 'stale flag cleared: no instant completion (修复前直接跳到 q_nuclear)');
      api.setFlag('newPlanet', true); api.pokeQuests();   // 真正再次着陆
      A.eq(api.questId(), 'q_nuclear', 'completes on fresh landing');
    });
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

  t.test('q_warp 不因消耗电池而永久卡死（结算先于扣电池，站内购买即时结算）', async function () {
    function toWarp() {
      api.clearInv();
      api.setFlag('checkedShip', true); api.pokeQuests();
      api.give('carbon', 15); api.pokeQuests();
      api.give('sodium', 8); api.pokeQuests();
      api.give('stone', 12); api.pokeQuests();
      api.placeEvent('furnace');
      api.give('iron', 10); api.pokeQuests();
      api.setFlag('shipRepaired', true); api.pokeQuests();
      api.give('data', 99); api.research('metallurgy');
      api.placeEvent('miner');
      for (var i = 0; i < 6; i++) api.placeEvent('belt');
      api.placeEvent('solar'); api.placeEvent('solar');
      api.placeEvent('refinery');
      api.give('fuel', 2); api.pokeQuests();
      api.setFlag('launched', true); api.pokeQuests();
      api.setFlag('docked', true); api.pokeQuests();
      api.setFlag('traded', true); api.pokeQuests();
      api.setFlag('newPlanet', true); api.pokeQuests();
      api.placeEvent('reactor');
      api.give('antimatter', 3); api.pokeQuests();
      A.eq(api.questId(), 'q_warp', 'arrived at q_warp');
    }

    // 路径 A：站内购买电池的瞬间（交易回调 → checkQuest）即结算收集任务
    await api.boot('normal', { fresh: true });
    toWarp();
    api.give('warpcell', 1);
    window.Game.checkQuest();
    A.eq(api.questId(), 'q_leave', 'holding a warpcell completes q_warp at check time');

    // 路径 B：电池未经结算直接跃迁消耗（旧行为 → q_warp 永久卡死）
    await api.boot('normal', { fresh: true });
    toWarp();
    api.give('warpcell', 1);   // 模拟「站内购买后未落地」：不触发任何任务轮询
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Game.warpTo(9999);   // 跃迁消耗电池
    A.eq(api.questId(), 'q_leave', 'warp consumes cell AFTER quest settles');
    A.eq(api.count('warpcell'), 0, 'cell consumed by warp');
    // 主线终点在跃迁后可正常完结
    api.setFlag('warpedOut', true); api.pokeQuests();
    A.eq(api.questId(), null, 'quest line completes after warp');
    await window.Game.tpTo(0, null, 'planet', 'reset');   // 复位状态，避免污染后续套件
  });

  t.test('村庄委托：接受 → 交付领赏 → 未足提示', function () {
    return api.boot('normal', { fresh: true }).then(function () {
      // 清除 newGame 赠送的起始物资（碳×10 钠×5）：委托物品随机抽取，
      // 若抽到碳/钠，剩余起始物资会破坏“物品扣除归零”与“不足时保持进行中”的判定
      api.clearInv();
      // 第一次对话：无委托 → 发放新委托并持久化到旗标
      var sq = window.Game.debugSideQuestTalk();
      A.ok(sq && sq.item && sq.need > 0 && sq.reward > 0, 'side quest offered: ' + JSON.stringify(sq));
      A.ok(window.Game.flags.sideQuest, 'quest persisted in flags');
      // 集齐物品 → 交付：物品扣除 + 星币入账 + 委托清空
      var before = api.credits();
      api.give(sq.item, sq.need);
      var sq2 = window.Game.debugSideQuestTalk();
      A.eq(sq2, null, 'quest cleared after delivery');
      A.eq(api.credits(), before + sq.reward, 'reward credited');
      A.eq(api.count(sq.item), 0, 'items taken');
      A.eq(window.Game.flags.sideQuest || null, null, 'flag cleared');
      // 再次对话：新委托；物品不足 → 保持进行中、不扣款
      var sq3 = window.Game.debugSideQuestTalk();
      A.ok(sq3, 'new quest offered');
      api.give(sq3.item, Math.max(0, sq3.need - 1));
      var cr = api.credits();
      var sq4 = window.Game.debugSideQuestTalk();
      A.ok(sq4 && sq4.item === sq3.item, 'insufficient: quest stays active');
      A.eq(api.credits(), cr, 'no reward for insufficient delivery');
      window.Game.debugCloseDialog();
    });
  });
});

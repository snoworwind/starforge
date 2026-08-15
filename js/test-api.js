/* ============================================================
   STARFORGE - test-api.js
   全自动测试接口：暴露 window.__SF_TEST__ 给 AI agent / Playwright /
   浏览器控制台，用于驱动游戏、断言状态、收集机器可读结果。

   加载方式：
     - 打开 index.html?test=1 自动加载（index.html 末尾条件加载器）
     - 或手动 <script src="js/test-api.js"></script> / 控制台注入

   核心目标：无人值守持续迭代 —— 一次 runAll() 得到完整 JSON 结果。
   ============================================================ */
(function () {
  'use strict';
  if (window.__SF_TEST__) return;   // 幂等：重复注入不覆盖

  // ---------- 确定性随机（测试自带的 mulberry32，不依赖游戏全局） ----------
  function mulberry32(seed) {
    let a = seed >>> 0;
    return function () {
      a |= 0; a = (a + 0x6D2B79F5) | 0;
      let t = Math.imul(a ^ (a >>> 15), 1 | a);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }

  // ---------- 工具 ----------
  const sleep = (ms) => new Promise(r => setTimeout(r, ms));
  function deepClone(x) {
    if (x === null || typeof x !== 'object') return x;
    if (x && x.isVector3) return [x.x, x.y, x.z];
    if (Array.isArray(x)) return x.map(deepClone);
    const o = {};
    for (const k in x) { if (Object.prototype.hasOwnProperty.call(x, k)) o[k] = deepClone(x[k]); }
    return o;
  }
  function json(x) { return JSON.parse(JSON.stringify(x)); }
  function fmt(v) { return typeof v === 'string' ? JSON.stringify(v) : JSON.stringify(v); }

  // ---------- 测试断言 ----------
  class AssertionError extends Error {}
  const A = {
    ok(v, msg) { if (!v) throw new AssertionError(msg || 'expected truthy, got ' + fmt(v)); },
    eq(a, b, msg) { if (a !== b) throw new AssertionError((msg || 'not equal') + ' — got ' + fmt(a) + ', want ' + fmt(b)); },
    ne(a, b, msg) { if (a === b) throw new AssertionError((msg || 'unexpectedly equal') + ' — both ' + fmt(a)); },
    gt(a, b, msg) { if (!(a > b)) throw new AssertionError((msg || 'expected >') + ' — got ' + fmt(a) + ' > ' + fmt(b) + ' ?'); },
    ge(a, b, msg) { if (!(a >= b)) throw new AssertionError((msg || 'expected >=') + ' — got ' + fmt(a) + ' >= ' + fmt(b) + ' ?'); },
    lt(a, b, msg) { if (!(a < b)) throw new AssertionError((msg || 'expected <') + ' — got ' + fmt(a) + ' < ' + fmt(b) + ' ?'); },
    between(v, lo, hi, msg) { if (!(v >= lo && v <= hi)) throw new AssertionError((msg || 'out of range') + ' — got ' + fmt(v) + ' want [' + lo + ',' + hi + ']'); },
    throws(fn, msg) { try { fn(); } catch (e) { return e; } throw new AssertionError(msg || 'expected to throw'); },
    match(s, re, msg) { if (!re.test(s)) throw new AssertionError((msg || 'no match') + ' — ' + fmt(s) + ' vs ' + re); },
  };

  // ---------- 音频中性化（测试确定性关键） ----------
  // Sound.begin 会启动 900ms 的 Music setInterval，其内部调用 Math.random，
  // 在 boot 的「挂种子随机」窗口内按墙钟时间消耗随机数 → 世界种子不再确定。
  // 测试无需音频：把 Sound 全部打成空操作，消除背景随机消费与 AudioContext 依赖。
  function neutralizeAudio() {
    try {
      const S = (typeof Sound !== 'undefined') ? Sound : window.Sound;
      if (!S) return;
      S.play = function () {};
      S.begin = function () {};
      S.resume = function () {};
      S.setVolume = function () {};
      if (S.Music) { S.Music.start = function () {}; S.Music.stop = function () {}; S.Music.setMode = function () {}; }
      if (S.loops) for (const k in S.loops) {
        const l = S.loops[k];
        if (l && typeof l === 'object') { l.start = function () {}; l.stop = function () {}; l.set = function () {}; }
      }
    } catch (e) {}
  }

  // ---------- 错误捕获（记录测试期间任何未捕获异常） ----------
  const errLog = [];
  function hookErrors() {
    if (window.__sfTestErrHooked) return;
    window.__sfTestErrHooked = true;
    window.addEventListener('error', e => {
      errLog.push('[error] ' + ((e.message || '') + ' @' + ((e.filename || '').split('/').pop() || '') + ':' + (e.lineno || 0)));
    });
    window.addEventListener('unhandledrejection', e => {
      errLog.push('[unhandledrejection] ' + ((e.reason && (e.reason.stack || e.reason.message)) || e.reason));
    });
  }

  // ---------- 全局词法绑定（data.js 等 classic script 顶层 const/let）----------
  const G = {
    get RECIPES() { return RECIPES; }, get RECIPE_BY_ID() { return RECIPE_BY_ID; },
    get BLOCKS() { return BLOCKS; }, get BLOCK_BY_ID() { return BLOCK_BY_ID; },
    get ITEMS() { return ITEMS; }, get TECH() { return TECH; },
    get BIOMES() { return BIOMES; }, get QUESTS() { return QUESTS; },
    get TRADE_GOODS() { return TRADE_GOODS; }, get STATION_BLUEPRINTS() { return STATION_BLUEPRINTS; },
    get SYSTEM_PLANETS() { return SYSTEM_PLANETS; },
    get DEFAULT_PLANETS() { return DEFAULT_PLANETS; },
    get HOME_GALAXY_SEED() { return HOME_GALAXY_SEED; },
    get CREATURE_TYPES() { return CREATURE_TYPES; },
  };

  // ---------- 启动：确定性生成星球 ----------
  let currentMode = null;      // 'creative' | 'normal' | 'easy' | 'hard'
  let bootSeed = 12345;
  const _origRandom = Math.random;

  function normalizeMode(mode) {
    const m = String(mode || 'normal').toLowerCase();
    if (['creative', 'easy', 'normal', 'hard', 'survival'].includes(m)) return m === 'survival' ? 'normal' : m;
    throw new Error('unknown mode: ' + mode);
  }
  function triggerNewGame(mode) {
    const btn = mode === 'creative' ? 'btnCreative'
      : mode === 'easy' ? 'btnDiffEasy'
      : mode === 'hard' ? 'btnDiffHard'
      : 'btnDiffNormal';
    const el = document.getElementById(btn);
    if (!el || !el.onclick) throw new Error('menu button missing: ' + btn);
    el.onclick();
    // 新流程：难度 → 捏人 → 世界创建（测试自动走完真实 UI 链路）
    const nameInput = document.getElementById('charNameInput');
    if (nameInput){
      nameInput.value = '测试旅行者';
      const cf = document.getElementById('btnCharConfirm');
      if (!cf || !cf.onclick) throw new Error('char confirm button missing');
      cf.onclick();
    }
    const wName = document.getElementById('worldNameInput');
    if (wName){
      wName.value = '测试世界';
      const wc = document.getElementById('btnWorldConfirm');
      if (!wc || !wc.onclick) throw new Error('world confirm button missing');
      wc.onclick();
    }
  }
  async function waitUntil(fn, timeout, step) {
    timeout = timeout || 60000; step = step || 25;
    const t0 = Date.now();
    while (Date.now() - t0 < timeout) {
      let v = false;
      try { v = fn(); } catch (e) { v = false; }
      if (v) return true;
      await sleep(step);
    }
    throw new Error('waitUntil timeout: ' + String(fn).slice(0, 90));
  }

  async function boot(mode, opts) {
    hookErrors();
    mode = normalizeMode(mode);
    opts = opts || {};
    if (!window.Game || !window.World || !window.Player) throw new Error('game not loaded');
    if (currentMode === mode && !opts.fresh) return snapshot();

    const seed = opts.seed != null ? opts.seed : bootSeed;
    Math.random = mulberry32(seed);        // 生成期挂种子随机（含世界种子/出生点）
    try {
      triggerNewGame(mode);
    } catch (e) {
      Math.random = _origRandom;
      throw e;
    }
    try {
      await waitUntil(() => window.Game.state === 'planet', 90000, 30);
    } finally {
      Math.random = _origRandom;           // 星球生成完毕即还原
    }
    currentMode = mode;
    // 创造模式：全科技已解锁；生存：仅 survival。dropMult 由 Game 反映。
    return snapshot();
  }
  function reboot(mode, opts) { return boot(mode, Object.assign({}, opts, { fresh: true })); }

  // ---------- 状态快照 / 查询 ----------
  function questIdxValue() {
    const id = window.Game.currentQuestId();
    if (id == null) return QUESTS.length;
    const i = QUESTS.findIndex(q => q.id === id);
    return i < 0 ? QUESTS.length : i;
  }
  function snapshot() {
    return {
      state: window.Game.state,
      creative: window.Game.creative,
      dropMult: window.Game.dropMult,
      currentPlanet: window.Game.currentPlanet,
      worldSeed: window.World.seed,
      biome: window.World.biome ? window.World.biome.name : null,
      planetName: (SYSTEM_PLANETS[window.Game.currentPlanet] || {}).name || null,
      credits: window.Player.credits,
      stats: json(window.Player.stats),
      inv: window.Player.inv.map(s => s ? { item: s.item, n: s.n } : null),
      hotIdx: window.Player.hotIdx,
      pos: [window.Player.pos.x, window.Player.pos.y, window.Player.pos.z],
      questIdx: questIdxValue(),
      questId: window.Game.currentQuestId(),
      tech: techList(),
      flags: json(window.Game.flags),
      machines: window.Factory ? window.Factory.machines.size : 0,
      power: window.Factory ? json(window.Factory.power) : null,
    };
  }

  // ---------- 背包 ----------
  function clearInv() { window.Player.inv.fill(null); window.UI && window.UI.refreshAll(); }
  function give(id, n) { return window.Player.addItem(id, n == null ? 1 : n, true); }
  function take(id, n) { return window.Player.removeItem(id, n == null ? 1 : n); }
  function count(id) { return window.Player.countItem(id); }
  function has(id, n) { return window.Player.countItem(id) >= (n == null ? 1 : n); }
  function inv() { return json(window.Player.inv); }

  // ---------- 玩家 ----------
  function setPos(x, y, z) { window.Player.pos.set(x, y, z); window.Player.vel.set(0, 0, 0); }
  function pos() { return [window.Player.pos.x, window.Player.pos.y, window.Player.pos.z]; }
  function setStat(k, v) { window.Player.stats[k] = v; }
  function setCredits(n) { window.Player.credits = n; }

  // ---------- 世界 ----------
  function blockKeyAt(x, y, z) { return window.World.getDef(x, y, z).key; }
  function setBlock(x, y, z, key) { window.World.set(x, y, z, BLOCKS[key].id); return true; }
  function topAt(x, z) { return window.World.topAt(x, z); }
  function findSpawn() { const v = window.World.findSpawn(); return [v.x, v.y, v.z]; }

  // ---------- 合成（镜像 UI.tryCraft，可精确断言产出） ----------
  function canCraft(recipeId) {
    const r = RECIPE_BY_ID[recipeId];
    if (!r) return false;
    if (r.where !== 'hand' && r.where !== 'both') return false;
    if (r.tech && !window.Game.techDone(r.tech)) return false;
    return window.Player.hasItems(r.in);
  }
  function craft(recipeId, n) {
    const r = RECIPE_BY_ID[recipeId];
    if (!r) throw new Error('no recipe: ' + recipeId);
    if (r.where !== 'hand' && r.where !== 'both') throw new Error('not portable: ' + recipeId);
    let made = 0;
    n = n == null ? 1 : n;
    for (let i = 0; i < n; i++) {
      if (r.tech && !window.Game.techDone(r.tech)) break;
      if (!window.Player.hasItems(r.in)) break;
      window.Player.payItems(r.in);
      for (const k in r.out) window.Player.addItem(k, r.out[k] * window.Game.dropMult, true);
      made++;
    }
    return made;
  }

  // ---------- 科技 ----------
  function techList() {
    const out = [];
    for (const id in TECH) if (window.Game.techDone(id)) out.push(id);
    return out;
  }
  function canResearch(id) {
    const t = TECH[id];
    if (!t) return false;
    if (window.Game.techDone(id)) return false;
    if (!t.req.every(r => window.Game.techDone(r))) return false;
    return window.Player.hasItems(t.cost);
  }
  function research(id) {
    const t = TECH[id];
    if (!t) throw new Error('no tech: ' + id);
    if (window.Game.techDone(id)) return true;
    if (!t.req.every(r => window.Game.techDone(r))) return false;
    if (!window.Player.payItems(t.cost)) return false;
    window.Game.completeTech(id);
    return true;
  }
  // 走真实计时研究：设置 UI.researching 并推进 updateResearch
  async function researchTimed(id, dt) {
    const t = TECH[id];
    if (!t) throw new Error('no tech: ' + id);
    if (window.Game.techDone(id)) return true;
    if (!t.req.every(r => window.Game.techDone(r))) throw new Error('prereq not met: ' + id);
    if (!window.Player.payItems(t.cost)) throw new Error('cannot afford: ' + id);
    window.UI.researching = { id, t: 0 };
    const step = dt || 0.05;
    for (let acc = 0; acc < (t.time || 1) * 2 + 1; acc += step) {
      window.UI.updateResearch(step);
      if (window.Game.techDone(id)) break;
      await sleep(1);
    }
    return window.Game.techDone(id);
  }

  // ---------- 工厂 ----------
  function blockKeyForMachine(type) {
    for (const k in BLOCKS) if (BLOCKS[k].machine === type) return k;
    return null;
  }
  function placeMachine(type, x, y, z, dir) {
    const k = blockKeyForMachine(type);
    if (!k) throw new Error('no machine block for type: ' + type);
    return window.Factory.place(x, y, z, k, dir == null ? 0 : dir);
  }
  function removeMachine(x, y, z) { return window.Factory.remove(x, y, z); }
  function machineAt(x, y, z) {
    const m = window.Factory.at(x, y, z);
    return m ? { x: m.x, y: m.y, z: m.z, type: m.type, dir: m.dir, data: json(m.data) } : null;
  }
  function machines() {
    const out = [];
    for (const m of window.Factory.machines.values())
      out.push({ x: m.x, y: m.y, z: m.z, type: m.type, dir: m.dir, data: json(m.data) });
    return out;
  }
  function machineInsert(x, y, z, item) { return window.Factory.machineInsert(window.Factory.at(x, y, z), item); }
  function machineAccept(x, y, z, item) { return window.Factory.canMachineAccept(window.Factory.at(x, y, z), item); }
  function setMachineRecipe(x, y, z, recipeId) {
    const m = window.Factory.at(x, y, z);
    if (!m) throw new Error('no machine at ' + x + ',' + y + ',' + z);
    m.data.recipe = recipeId;
  }
  function tickFactory(dt, day) { window.Factory.update(dt, day == null ? 1 : day); }
  function power() { return json(window.Factory.power); }

  // ---------- 任务 ----------
  function quest() {
    const id = window.Game.currentQuestId();
    if (id == null) return null;
    const q = QUESTS.find(x => x.id === id);
    return q ? json(q) : null;
  }
  function setFlag(name, v) { window.Game.flags[name] = v; }
  // 触发一次任务重评估（checkQuest 通过 onBlockMined/onBlockPlaced 等出口被调用）
  function pokeQuests() { window.Game.onBlockMined(); }
  // 模拟「放置方块」任务事件（place 类任务的 placedCount 计数入口）
  function placeEvent(blockKey) { window.Game.onBlockPlaced(blockKey); }

  // ---------- 存档 ----------
  function save(name) { return window.Game.saveTo(null, name || ('test_' + Date.now())); }
  function saveTo(key, name) { return window.Game.saveTo(key, name); }
  function load(key) { return window.Game.loadFrom(key); }
  function listSaves() { return window.Game.listSaves(); }
  function listChars() { return window.Game.listChars(); }
  function listWorlds() { return window.Game.listWorlds(); }
  function createCharacter(name, appearance) { return window.Game.createCharacter(name, appearance); }
  function deleteChar(key) { return window.Game.deleteChar(key); }
  function deleteWorld(key) { return window.Game.deleteWorld(key); }
  function loadPair(charKey, worldKey) { return window.Game.loadPair(charKey, worldKey); }
  function deleteSave(key) { return window.Game.deleteSave(key); }

  // ---------- 太空 / 星系（数据层确定性 + 冒烟） ----------
  function enterSpace() {
    window.Space.enter(window.Game.currentPlanet);
    return true;
  }
  function spaceState() {
    return {
      seed: window.Space.getCurrentGalaxySeed(),
      planets: window.Space.planets.map(p => ({ id: p.def.id, name: p.def.name, biome: p.def.biome })),
      ship: json(window.Space.shipState),
    };
  }
  function generateGalaxy(seed) {
    const g = window.generateGalaxy(seed);   // data.js 的全局函数声明（挂在 window 上）
    return { seed: g.seed, name: g.name, planets: g.planets.length, station: g.station, market: g.market };
  }

  // ---------- 测试框架 ----------
  const suites = [];
  function suite(name, fn) {
    const s = { name, tests: [], before: null, after: null };
    suites.push(s);
    const ctx = {
      test(tname, tfn) { s.tests.push({ name: tname, fn: tfn }); },
      before(f) { s.before = f; },
      after(f) { s.after = f; },
    };
    fn(ctx, api);
  }

  async function runAll(opts) {
    opts = opts || {};
    hookErrors();
    const results = [];
    const t0 = Date.now();
    for (const s of suites) {
      if (opts.grep && !s.name.match(opts.grep)) continue;
      const sr = { name: s.name, passed: 0, failed: 0, durationMs: 0, tests: [] };
      const s0 = Date.now();
      try { if (s.before) await s.before(api); } catch (e) { sr.beforeError = String(e && e.message || e); }
      for (const t of s.tests) {
        const e0 = errLog.length;
        const tt0 = performance.now();
        const tr = { name: t.name, pass: false, error: null, ms: 0 };
        try {
          await t.fn(api, A);
          tr.pass = true;
        } catch (e) {
          tr.error = String(e && e.message || e);
          if (errLog.length > e0) tr.error += ' | page: ' + errLog.slice(e0).join(' | ');
        }
        tr.ms = Math.round((performance.now() - tt0) * 100) / 100;
        sr.tests.push(tr);
        if (tr.pass) sr.passed++; else sr.failed++;
      }
      try { if (s.after) await s.after(api); } catch (e) { sr.afterError = String(e && e.message || e); }
      sr.durationMs = Date.now() - s0;
      results.push(sr);
    }
    const total = results.reduce((a, s) => a + s.tests.length, 0);
    const passed = results.reduce((a, s) => a + s.passed, 0);
    const failed = total - passed;
    return {
      generatedAt: new Date().toISOString(),
      totalMs: Date.now() - t0,
      summary: { suites: results.length, tests: total, passed, failed, ok: failed === 0 },
      suites: results,
      pageErrors: errLog.slice(),
    };
  }

  // ---------- 公开 API ----------
  const api = {
    version: '1.0.0',
    ready: true,
    // 框架
    suite, describe: suite,
    runAll, run: runAll,
    get results() { return api; },
    // 断言
    assert: A, ok: A.ok, eq: A.eq, ne: A.ne, gt: A.gt, ge: A.ge, lt: A.lt, between: A.between, throws: A.throws, match: A.match,
    // 启动
    boot, reboot, get mode() { return currentMode; }, setSeed(s) { bootSeed = s; },
    // 工具
    waitUntil, sleep, deepClone, json, mulberry32,
    // 查询
    snapshot, state() { return window.Game.state; },
    worldSeed() { return window.World.seed; },
    biome() { return window.World.biome ? window.World.biome.name : null; },
    currentPlanet() { return window.Game.currentPlanet; },
    credits() { return window.Player.credits; }, setCredits,
    // 背包
    give, take, count, has, clearInv, inv,
    hotIdx() { return window.Player.hotIdx; }, setHotIdx(n) { window.Player.hotIdx = n; },
    // 玩家
    pos, setPos, stats() { return json(window.Player.stats); }, setStat,
    damage(n) { window.Player.damage(n); }, dead() { return window.Player.dead; },
    recharge(kind) { return window.Player.recharge(kind); },
    chargeStat(kind) { return window.Player.chargeStat(kind); },
    canCharge(kind) { return window.Player.canCharge(kind); },
    // 世界
    blockKeyAt, setBlock, topAt, findSpawn,
    raycast(o, d, dist) { return window.World.raycast(new THREE.Vector3(o[0], o[1], o[2]), new THREE.Vector3(d[0], d[1], d[2]), dist == null ? 6 : dist); },
    // 合成
    craft, canCraft,
    // 科技
    research, researchTimed, canResearch, tech(id) { return window.Game.techDone(id); }, techList,
    // 工厂
    placeMachine, removeMachine, machineAt, machines, machineInsert, machineAccept, setMachineRecipe, tickFactory, power,
    // 任务
    quest, questId() { return window.Game.currentQuestId(); }, questIdx: questIdxValue, quests() { return window.Game.currentQuests(); },
    setFlag, pokeQuests, placeEvent, flag(name) { return window.Game.flags[name]; },
    // 存档
    save, saveTo, load, listSaves, deleteSave, listChars, listWorlds, createCharacter, deleteChar, deleteWorld, loadPair,
    // 太空 / 星系
    enterSpace, spaceState, generateGalaxy,
    galaxySeed() { return window.Space.getCurrentGalaxySeed(); },
    // 数据定义只读访问（便于测试断言，不污染游戏）
    defs: G,
  };

  neutralizeAudio();

  window.__SF_TEST__ = api;
  window.__SF_TEST_READY__ = true;
  console.log('[test-api] STARFORGE test interface ready (v' + api.version + ')');
})();

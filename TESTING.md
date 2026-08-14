# STARFORGE 自动化测试系统 — AI Agent 完整使用手册

> **目标读者**：AI agent / CI 流水线。本文是「读懂即可自主运行、扩展、排障、迭代」的完整手册。
> 读完本手册后，你应当能：① 一键跑通全部测试并解读结果；② 为任意新功能新增测试；③ 扩展测试接口；④ 判断并修复失败；⑤ 不破坏确定性。

---

## 0. 一句话总结

游戏本体是纯浏览器 Three.js 游戏（无构建系统）。测试方案 = **用 Playwright 无头启动真实浏览器 → 加载 `index.html?test=1`（自动注入测试接口 `window.__SF_TEST__`）→ 注入 `tests/*.js` 套件 → 在页面内执行 → 收集 JSON/JUnit 结果**。全程无需人工、无需真实 GPU（软件渲染 WebGL）、无需启动游戏的 `server.ps1`。

---

## 1. 目录结构与文件清单

```
wrsk/
├── index.html                  # 游戏入口；末尾有条件加载器（仅 ?test 时加载 test-api.js）
├── js/
│   ├── test-api.js             # ★ 测试接口本体（window.__SF_TEST__，约 60 个方法 + 微型测试框架）
│   ├── data.js                 # 数据定义：BLOCKS/ITEMS/RECIPES/TECH/QUESTS/BIOMES/星系
│   ├── world.js  factory.js  player.js  ui.js  main.js
│   ├── space.js  station.js  net.js  creatures.js  savestore.js  audio.js
│   └── (three.min.js / GLTFLoader.js / SVGLoader.js / models.js / modellib.js / textures.js)
├── tests/
│   ├── 01-data.js … 10-galaxy-space.js   # ★ 10 个测试套件（文件名排序 = 执行顺序）
├── test/
│   ├── run.mjs                 # ★ Node 编排器（Playwright 无头运行）
│   └── serve.mjs               # 测试专用静态服务器（端口 17899）
├── package.json                # scripts + playwright-core 依赖
├── TESTING.md                  # 本手册
├── test-results/               # 运行产物（gitignore）：test-results.json / .xml
└── .gitignore                  # 忽略 node_modules/ 与 test-results/
```

**改动边界（重要）**：对游戏本体的唯一改动是 `index.html` 末尾的**条件加载器**——URL 不带 `?test` 时它完全不执行，正常游玩零影响。测试接口、套件、运行器全部是**独立新增文件**，未侵入游戏逻辑。

---

## 2. 快速开始

前置条件：本机装有 **Node ≥ 18** 和 **Microsoft Edge（或 Chrome）**。首次执行：

```bash
npm install        # 安装 playwright-core（约几 MB，无需下载浏览器）
npm test           # 无头运行全套
```

其他命令：

```bash
npm run test:headed    # 有头模式（可视化观察游戏被驱动）
npm run test:factory   # 只跑名字匹配的套件（等价 node test/run.mjs --grep=factory）
node test/run.mjs --grep=^(data|world)$   # 正则过滤套件名
node test/run.mjs --browser=chrome        # 用 Chrome（默认 edge）
```

### 2.1 输出与退出码

- 终端打印：每个套件 `[✓]/[✗]` + 失败用例的错误信息。
- `test-results/test-results.json`：机器可读完整结果（结构见 §12.2）。
- `test-results/test-results.xml`：JUnit 格式（CI 直接吞）。
- **退出码 `0` = 全部通过；`1` = 有失败或致命异常**。CI 用退出码判定即可。

预期基线（当前）：**10 套件 · 60 用例 · 60 通过 · 约 20 秒**。

---

## 3. 整体流程（从命令到 JSON 的完整链路）

```
node test/run.mjs
  1. import test/serve.mjs        → 启动 http://127.0.0.1:17899 静态服务器（副作用：listen）
  2. chromium.launch(channel:'msedge', headless, args:[--enable-unsafe-swiftshader, --use-angle=swiftshader, --use-gl=angle])
  3. context.addInitScript        → 在页面任何脚本前写 localStorage.starforge_settings（低画质降载）
  4. page.goto('.../index.html?test=1')
     └─ index.html 末尾条件加载器检测到 ?test → append <script src="js/test-api.js">
     └─ test-api.js 载入：暴露 window.__SF_TEST__，并 neutraliseAudio()（见 §9）
  5. page.waitForFunction(窗口.__SF_TEST__.ready)
  6. 按文件名排序读 tests/*.js → page.addScriptTag({content}) 逐个注入
     └─ 每个文件调用 __SF_TEST__.suite(name, fn) 注册套件
  7. page.evaluate(() => __SF_TEST__.runAll({grep}))
     └─ runAll 顺序执行每个套件的 before → 各 test → after，收集 pass/fail/耗时/pageErrors
  8. 写 test-results.json + test-results.xml，打印摘要，process.exit(0|1)
```

**低画质预设**（第 3 步注入的 localStorage）：

```js
{ fov:75, chunkDist:6, farDist:400, quality:'low',
  planetLod:'low', clouds:'off', realAtmo:'off', npcShips:0 }
```

它让无头软件渲染跑得快且稳。不要删，否则 GPU 负载暴涨、易超时。

---

## 4. 前置：游戏代码结构（测试必须知道的模块与全局）

测试套件运行在**真实游戏环境**里，直接调用游戏暴露的模块。扩展测试前必须理解下面几点，否则会写出无法运行的代码。

### 4.1 全局对象 vs `window` 属性（★ 最容易踩的坑）

游戏脚本是**经典 `<script>`（非 ES module）**，顶层声明分两类，访问方式不同：

| 声明方式 | 例子 | 裸名访问 | `window.X` 访问 |
|---|---|---|---|
| `const X = ...` / `let X = ...`（顶层词法绑定） | `BLOCKS, ITEMS, RECIPES, RECIPE_BY_ID, TECH, QUESTS, BIOMES, CREATURE_TYPES, TRADE_GOODS, STATION_BLUEPRINTS, SYSTEM_PLANETS, DEFAULT_PLANETS, HOME_GALAXY_SEED, Sound, Tex, Icons` | ✅ | ❌（`undefined`） |
| `function X(...){}`（函数声明） | `mulberry32, makeNoise, generateGalaxy, galaxyName, resetGalaxy, setGalaxy` | ✅ | ✅ |
| `const X = (()=>{...})(); window.X = X`（IIFE 导出） | `Game, Player, World, Factory, UI, Space, Station, Net, SaveStore, Creatures, ModelLib` | ✅ | ✅ |

**结论**：
- 游戏本体模块（`Game/Player/World/Factory/UI/Space/SaveStore` 等）用 `window.X` 访问（它们同时是裸名可访问）。
- **数据定义（`BLOCKS/ITEMS/RECIPES/TECH/QUESTS/BIOMES/...`）是 `const/let` 词法绑定，只能用裸名，不能用 `window.X`**。这是当初 `window.BLOCKS` 报 `undefined` 的根源。
- `test-api.js` 已在内部用裸名 + 一个只读代理 `api.defs`（见 §5.14）把数据定义暴露给测试，**套件里请用 `api.defs.XXX` 而不是裸名或 `window.XXX`**，更安全清晰。

### 4.2 关键模块 API 一览（测试常用）

以下列出各模块 `return` 暴露的成员（测试会直接/间接用到）：

- **`window.Game`**：`state, flags, market, lastTech, creative, dropMult, currentPlanet, dayTime, shipPos, planetScene, techDone(id), completeTech(id), currentQuests(), currentQuestId(), onBlockMined(def), onBlockPlaced(key), lockPointer(), save(), saveTo(key,name), loadFrom(key), listSaves(), deleteSave(key), joinGame(init), addCargo(id,n), doScan(), setWarpLock(seed,name), isGalaxyVisited(seed), warpTo(seed), neighborSeeds(), saveBeaconState(pid)`。
- **`window.Player`**：`pos(Vector3), vel, stats{hp,shield,o2,haz,jet,laser 及 Max}, inv[36], hotIdx, credits, dead, keys, addItem(id,n,silent), removeItem(id,n), countItem(id), hasItems(costs), payItems(costs), damage(n), recharge(kind), chargeStat(kind), canCharge(kind), CHARGE_DEFS, serialize(), deserialize(d), findSpawn 无, spawnDrop, spawnParticles, tryPlace(camera), lookTarget(camera), update(dt,camera)`。
  - ⚠️ `Player.countItem/removeItem/hasItems/payItems` 在 `main.js` 里被**重包装**，会同时统计「飞船货仓」；测试中飞船货仓恒空，等价于随身背包，但要知道这个事实。
- **`window.World`**：`biome, seed, group, materials, farMesh, init(biomeKey,seed,mods), pregen(x,z,r,cb), stream(x,z), update(dt,x,z), get(x,y,z)→blockId, set(x,y,z,id,silent), getDef(x,y,z)→BLOCKS条目, raycast(origin,dir,maxDist), topAt(x,z), findSpawn()→Vector3, serialize()→{seed,mods}, dispose(), inBounds(x,y,z), setCurve, setScanPulse, surfaceColorAt, mapColorAt, mapColorRGB, mapHeightAt, setViewDist, setFarDist, setShadows, structures`。
- **`window.Factory`**：`init(scene), place(x,y,z,blockKey,dir), remove(x,y,z), at(x,y,z), update(dt,dayFactor), serialize(), deserialize(arr), reset(), canMachineAccept(m,item), machineInsert(m,item), power{gen,use,sat}, machines(Map), DIRS`。
- **`window.UI`**：`anyPanelOpen(), closeAll(), toggle(id), buildHotbar(), refreshInv(), refreshAll(), updateResearch(dt), researching(get/set), openMachinePanel, bigMessage, refreshQuests, refreshHUD, ...`。
- **`window.Space`**：`scene, planets, shipState{pos,speed,yaw,pitch,roll,pos...}, enter(planetId), getCurrentGalaxySeed(), restoreGalaxy(seed), warpGalaxy(seed), SHIP_CLASSES, SHIP_MODEL_NAMES, ...`。
- **`window.SaveStore`**：`open(), getSlot(key), putSlot(key,data), deleteSlot(key), getIndex(), putIndex(arr), atomicWrite(key,data,idx), isMigrated(), setMigrated(), available`。
- **`window.Station` / `window.Net` / `window.Creatures` / `window.ModelLib`**：较少被测试直接调用，需要时读各自源文件末尾的 `return {...}`。

### 4.3 数据定义（都在 `api.defs` 里）

- `BLOCKS`：方块定义（`id, name, solid, hard, drops, machine?, ore?, cross?, liquid?, ...`），`BLOCK_BY_ID` 反向索引。
- `ITEMS`：物品（`name, cat(res/mat/blk/mach/tool), stack, block?, price, ...`）。
- `RECIPES` / `RECIPE_BY_ID`：配方数组 / 按 id 索引。`where ∈ {hand, both, furnace, assembler, refinery}`；`in{...}` 消耗、`out{...}` 产出、`time` 秒、`tech?` 门槛、`hidden?`。
- `TECH`：科技（`cost{...}, time, pos, req[], desc, unlocked?`）。默认仅 `survival` 解锁。
- `QUESTS`：任务线（`id, title, type ∈ {collect, place, tech, event}, item/n, block, tech, flag, dialog`）。
- `BIOMES`：星球生态（`name, grass/dirt/deep, sky/fog, haz?, hazRate, trees/flowers, oreMul, tint, animal, ...`）。
- `SYSTEM_PLANETS` / `DEFAULT_PLANETS`：初始星系行星布局（`{id, biome, name, pos, radius}`）。
- `TRADE_GOODS` / `STATION_BLUEPRINTS`：商品表 / 蓝图店。

### 4.4 关键常量速查（写断言时直接引用）

| 类别 | 值 |
|---|---|
| 工厂电力消耗 `POWER_USE` | miner 8 · assembler 12 · refinery 20（kW） |
| 工厂发电 `POWER_GEN` | solar 10（×白天因子）· reactor 100 · burner 25 · wind 2..16 |
| 熔炉燃料 `FUEL_VALUE`（秒） | carbon 4 · coal 16 · planks_b 3 |
| 工厂 tick 步长 | `Factory.update` 内部按 `TICK=0.1s` 累积步进 |
| 机器朝向 `Factory.DIRS` | `0:+x, 1:+z, 2:-x, 3:-z`（`placeMachine(type,x,y,z,dir)` 的 dir） |
| 机器类型 `BLOCKS[k].machine` | furnace, miner, belt, assembler, solar, refinery, chest, reactor, launchpad, wind, burner, beacon, lumberbot, collector |
| 难度产出倍率 `dropMult` | creative 1 · easy 7 · normal 4 · hard 1 |
| 新游戏起始物资 | credits 250 · carbon 10 · sodium 5（`newGame` 固定赠送） |
| 常用堆叠上限 | 大部分 250；carbon/oxygen/sodium/coal/矿石/金属锭/gear/wire 等 250；fuel 20 · uranium 100 · tritium 500 · antimatter 10 · warpcell 10 · circuit/plate 200 · data 500 · lamp_b 100 |
| 世界高度 | `World.WORLD_H = 64`，`World.SEA = 20`，区块 `CHUNK = 16` |
| 起源星系种子 | `HOME_GALAXY_SEED = 7777` |

---

## 5. 测试接口完整参考（`window.__SF_TEST__`，即套件里的 `api`）

> 约定：方法若无 `await` 说明是同步的。`json(...)` = 深拷贝返回的普通对象，可安全断言。所有动作都作用于**当前已 boot 的星球状态**。

### 5.1 生命周期 / 启动

| 方法 | 说明 |
|---|---|
| `await api.boot(mode, opts?)` | 确定性生成星球。`mode ∈ 'creative'|'easy'|'normal'|'hard'|'survival'(=normal)`。`opts = { seed?, fresh? }`。返回快照对象。**缓存**：若 `mode` 与当前一致且非 `fresh`，直接返回现有快照、不重新生成。 |
| `await api.reboot(mode, opts?)` | `boot(mode, {...opts, fresh:true})`，强制全新重开。 |
| `api.mode` | 当前 boot 的模式（getter）。 |
| `api.setSeed(n)` | 设置 `boot` 的默认种子（默认 12345）。 |

> `boot` 内部：临时把 `Math.random` 换成 `mulberry32(seed)` → 触发对应难度的「开始新游戏」按钮 → 等 `Game.state === 'planet'` → 还原 `Math.random`。星球生成完即还原，故**生成后的世界完全由 `World.seed` 决定**，与后续墙钟随机无关。

### 5.2 状态查询

| 方法 | 返回 |
|---|---|
| `api.snapshot()` | 完整快照：`{state, creative, dropMult, currentPlanet, worldSeed, biome, planetName, credits, stats, inv[36], hotIdx, pos[x,y,z], questIdx, questId, tech[], flags, machines, power}` |
| `api.state()` | `Game.state`（`'menu'|'loading'|'planet'|'space'|'atmo'|...`） |
| `api.worldSeed()` / `api.biome()` / `api.currentPlanet()` | 世界种子 / 生态名 / 行星 id |
| `api.credits()` / `api.setCredits(n)` | 信用点读 / 写 |
| `api.stats()` | 玩家六维数值深拷贝 `{hp,shield,o2,haz,jet,laser 及 Max}` |
| `api.pos()` / `api.setPos(x,y,z)` | 玩家坐标读 / 写（写会清零速度） |
| `api.hotIdx()` / `api.setHotIdx(n)` | 快捷栏选中位（`-1` = 固定采矿激光位） |

### 5.3 背包

| 方法 | 返回 / 说明 |
|---|---|
| `api.give(id, n=1)` | `Player.addItem`，返回**实际入包数量**（受单格堆叠上限影响） |
| `api.take(id, n=1)` | `Player.removeItem`，返回 boolean（不够则 false 且不扣） |
| `api.count(id)` | 该物品总数（跨所有格） |
| `api.has(id, n=1)` | 是否持有 ≥ n |
| `api.clearInv()` | 清空 36 格并刷新 UI |
| `api.inv()` | 36 格深拷贝 `[ {item,n} | null ×36 ]` |

### 5.4 玩家 / 生存

| 方法 | 说明 |
|---|---|
| `api.setStat(k, v)` | 写 `stats[k]`（如 `'haz'`、`'shield'`） |
| `api.damage(n)` | 调 `Player.damage(n)`（先扣盾后扣血；创造模式无效） |
| `api.dead()` | 是否死亡 |
| `api.recharge(kind)` | `Player.recharge(kind)`（`'haz'|'o2'` 快捷补给） |
| `api.chargeStat(kind)` | `Player.chargeStat(kind)`（激光/护盾/生命/氧气/防护 通用充能） |
| `api.canCharge(kind)` | 是否可充能 |

### 5.5 世界

| 方法 | 说明 |
|---|---|
| `api.blockKeyAt(x,y,z)` | 该坐标方块 key（如 `'stone'`、`'air'`） |
| `api.setBlock(x,y,z,key)` | 写方块（按 `BLOCKS[key].id`） |
| `api.topAt(x,z)` | 地表最高固体方块 y（0..63） |
| `api.findSpawn()` | 出生点 `[x,y,z]` |
| `api.raycast(origin[3], dir[3], maxDist=6)` | 世界射线，命中返回 `{x,y,z,def,face,dist}`，否则 `null` |

### 5.6 合成

| 方法 | 说明 |
|---|---|
| `api.craft(recipeId, n=1)` | 镜像游戏内便携合成：逐份检查科技门槛 + 材料，`payItems` 后按 `dropMult` 放大产出。返回**实际产出份数**。`recipeId` 必须是 `where ∈ {hand, both}`。 |
| `api.canCraft(recipeId)` | 是否可合成（门槛 + 材料） |

### 5.7 科技

| 方法 | 说明 |
|---|---|
| `api.tech(id)` | 是否已解锁 |
| `api.techList()` | 已解锁科技 id 数组 |
| `api.canResearch(id)` | 前置满足 + 材料足够 + 未解锁 |
| `api.research(id)` | **立即**研究（校验前置 + 扣费 + `completeTech`），返回 boolean |
| `await api.researchTimed(id, dt=0.05)` | 走**真实计时**研究：设 `UI.researching` 后逐步 `UI.updateResearch(dt)` 直到完成 |

### 5.8 工厂

| 方法 | 说明 |
|---|---|
| `api.placeMachine(type, x, y, z, dir=0)` | 放置机器（`type` 见 §4.4 机器类型表；内部按 `BLOCKS[k].machine` 反查方块 key）。**必须先 boot（工厂已初始化）** |
| `api.removeMachine(x,y,z)` | 拆除 |
| `api.machineAt(x,y,z)` | `{x,y,z,type,dir,data}` 或 `null`（`data` 已深拷贝） |
| `api.machines()` | 全部机器数组 |
| `api.machineInsert(x,y,z,item)` | 向机器投 1 个物品，返回 boolean |
| `api.machineAccept(x,y,z,item)` | 机器能否接收该物品 |
| `api.setMachineRecipe(x,y,z,recipeId)` | 直接写 `data.recipe`（装配/精炼用） |
| `api.tickFactory(dt, day=1)` | 推进工厂模拟 `dt` 秒（内部 0.1s 步进），`day` 为白天因子（太阳能发电 = 10×day） |
| `api.power()` | `{gen, use, sat}` |

### 5.9 任务

| 方法 | 说明 |
|---|---|
| `api.questId()` | 当前任务 id（`null` = 已全部完成） |
| `api.questIdx()` | 当前任务索引（完成 = `QUESTS.length`） |
| `api.quest()` | 当前任务对象（深拷贝）或 `null` |
| `api.quests()` | `Game.currentQuests()` |
| `api.setFlag(name, v)` / `api.flag(name)` | 写 / 读任务旗标（`Game.flags`） |
| `api.pokeQuests()` | 触发一次任务重评估（内部 `Game.onBlockMined()`） |
| `api.placeEvent(blockKey)` | 模拟「放置方块」事件（place 类任务的 `placedCount` 计数入口） |

> **任务推进套路**：collect 类 = `give` 后 `pokeQuests()`；place 类 = `placeEvent(block)`；tech 类 = `research(id)`（内部会触发 `checkQuest`）；event 类 = `setFlag` 后 `pokeQuests()`。完整 21 步示例见 `tests/08-quests.js`。

### 5.10 存档

| 方法 | 返回 / 说明 |
|---|---|
| `await api.save(name?)` | 新建槽位存档（`saveTo(null, name)`），返回 boolean |
| `await api.saveTo(key, name?)` | 覆盖指定槽位 |
| `await api.load(key)` | 读档（`loadFrom`，异步完整重建星球），返回 boolean |
| `await api.listSaves()` | 槽位索引数组（按时间倒序） |
| `await api.deleteSave(key)` | 删除槽位 |

### 5.11 太空 / 星系

| 方法 | 说明 |
|---|---|
| `api.enterSpace()` | `Space.enter(currentPlanet)`（初始化太空场景 + 摆船） |
| `api.spaceState()` | `{seed, planets:[{id,name,biome}], ship}` |
| `api.galaxySeed()` | 当前星系种子 |
| `api.generateGalaxy(seed)` | 调 data.js 的纯函数 `generateGalaxy`，返回 `{seed, name, planets(数量), station, market}` |

### 5.12 断言（抛 `AssertionError` → 用例失败）

`api.assert` 是一个对象，同时 `api.eq/ok/ne/gt/ge/lt/between/throws/match` 是其快捷方法；`t.test(fn)` 的第二个参数 `A` 也是同一对象。

| 断言 | 语义 |
|---|---|
| `ok(v, msg?)` | 真值 |
| `eq(a, b, msg?)` | 严格相等（失败信息带 got/want） |
| `ne(a, b, msg?)` | 不相等 |
| `gt / ge / lt` | `>` / `>=` / `<` |
| `between(v, lo, hi, msg?)` | 闭区间 |
| `throws(fn, msg?)` | 期望 fn 抛错 |
| `match(str, regex, msg?)` | 正则匹配 |

### 5.13 工具

| 方法 | 说明 |
|---|---|
| `await api.waitUntil(fn, timeout=60000, step=25)` | 轮询直到 `fn()` 真值，超时抛错 |
| `await api.sleep(ms)` | 延时 |
| `api.deepClone(x)` / `api.json(x)` | 深拷贝 / JSON 深拷贝 |
| `api.mulberry32(seed)` | 测试自带的确定性随机函数（可自造确定性数据） |
| `api.runAll({grep?})` / `api.run(opts)` | 运行全部已注册套件（通常由 run.mjs 调用） |
| `api.suite(name, fn)` / `api.describe(name, fn)` | 注册套件 |

### 5.14 数据定义只读代理 `api.defs`

`api.defs.XXX` 可读：`RECIPES, RECIPE_BY_ID, BLOCKS, BLOCK_BY_ID, ITEMS, TECH, BIOMES, QUESTS, TRADE_GOODS, STATION_BLUEPRINTS, SYSTEM_PLANETS, DEFAULT_PLANETS, HOME_GALAXY_SEED, CREATURE_TYPES`。

> 用 `api.defs.ITEMS.carbon.stack` 之类读定义，**不要**在套件里裸写 `ITEMS`/`BLOCKS`（那是词法绑定，虽在当前全局脚本里也能解析，但用 `api.defs` 更统一、意图更清晰）。

---

## 6. 测试框架 DSL

```js
__SF_TEST__.suite('套件名', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); }); // 可选，可返回 Promise
  t.after(function () { /* 清理，可选 */ });

  t.test('用例名', function () {          // 同步
    A.eq(1 + 1, 2, '一加一');
  });

  t.test('异步用例', function () {        // 返回 Promise 即异步
    return api.boot('creative', { fresh: true }).then(function () {
      A.ok(api.tech('survival'));
    });
  });
});
```

要点：
- `suite(name, fn)`：`fn(t, api)` 立即执行，用于**注册**测试（不要在这里跑断言）。
- `t.test(name, fn)`：注册一个用例；`fn(api, A)` 在 `runAll` 时才执行。
- `t.before(fn)` / `t.after(fn)`：套件级钩子，`before` 在第一个用例前、`after` 在最后一个用例后；均支持 async。
- 用例**抛异常 = 失败**；`A.*` 断言内部抛 `AssertionError`。
- 用例失败时，`runAll` 会同时把该用例执行期间新增的页面错误（`pageerror`/`unhandledrejection`）拼进 `error` 字段。

---

## 7. 编写新测试（完整步骤）

1. 在 `tests/` 新建 `NN-xxx.js`（`NN` 两位数字决定执行顺序，例如 `11-combat.js`）。
2. 用 §6 的 DSL 写套件。
3. **选择 boot 模式**（见 §10）：需要干净世界/机器 → `fresh:true`；纯逻辑（背包/合成）可不 boot 或复用默认态。
4. 用 `api.defs` 读数据、用 `api.*` 驱动、用 `A.*` 断言。
5. `npm test` 或 `node test/run.mjs --grep=NN` 验证。

**模板 A：纯逻辑（不 boot，最快）**

```js
__SF_TEST__.suite('my-logic', function (t, api) {
  var A = api.assert;
  t.before(function () { api.clearInv(); api.setCredits(0); });
  t.test('合成一份木板', function () {
    api.give('carbon', 4);
    A.eq(api.craft('planks_b', 1), 1);
    A.eq(api.count('carbon'), 0);
  });
});
```

**模板 B：需要真实世界/工厂（boot）**

```js
__SF_TEST__.suite('my-world', function (t, api) {
  var A = api.assert;
  t.before(function () { return api.boot('normal', { fresh: true }); });
  t.test('放置并拆除熔炉', function () {
    var y = api.topAt(40, 40) + 1;
    api.placeMachine('furnace', 40, y, 40, 0);
    A.eq(api.machineAt(40, y, 40).type, 'furnace');
    api.removeMachine(40, y, 40);
    A.eq(api.machineAt(40, y, 40), null);
  });
});
```

**模板 C：异步流程（存档/读档/计时）**

```js
t.test('存档往返', function () {
  api.setCredits(500);
  return api.save('x').then(function (ok) {
    A.ok(ok);
    return api.listSaves();
  }).then(function (saves) {
    return api.load(saves[0].key);
  }).then(function (loaded) {
    A.ok(loaded);
    A.eq(api.credits(), 500);
  });
});
```

---

## 8. 扩展测试接口（新增动作/查询）

当游戏新增了子系统、现有 `api` 不够用时，在 `js/test-api.js` 内添加：

1. 在 IIFE 里写一个普通函数（可引用裸名数据定义如 `BLOCKS`，或 `window.Game` 等模块）。
2. 把函数加进文件末尾的 `const api = { ... }` 对象。
3. 保持函数**返回纯 JSON**（用 `json()`/`deepClone()` 包裹 THREE 对象或 Map），别把 live 引用暴露给套件。

示例（新增「设置飞船燃料」）：

```js
function setFuel(n) { window.Game; /* 视实现 */ }
// 在 api 对象里加： setFuel,
```

之后 `api.setFuel(3)` 即可在套件里用。规则：**动作作用于真实游戏状态，查询返回 JSON，异常抛错即失败**。

---

## 9. 确定性与隔离（★ 铁律，勿破坏）

1. **音频必须保持中性化**：`test-api.js` 载入即把 `Sound.*` 打成空操作。原因是 `Sound.begin()` 会启动 900ms 的 `Music` `setInterval`，其内部调 `Math.random`，会在 `boot` 的「挂种子随机」窗口内按**墙钟时间**消耗随机数，导致世界种子不再可复现。**不要在测试里重新启用音频或让任何 `setInterval` 在 boot 窗口内调 `Math.random`。**
2. **`boot` 的确定性边界**：只有 `Math.random` 被换掉的「星球生成窗口」内是确定性的；生成完成后 `Math.random` 还原，此后游戏主循环/生物的随机不可依赖。断言世界地形/种子用「同一 `seed` 两次 boot 结果相同」，不要断言与墙钟相关的东西。
3. **套件隔离**：涉及共享可变状态（世界方块、机器、科技、任务、存档）的套件，`before` 一律 `boot(mode, {fresh:true})` 全新重开；不要依赖前一套件留下的状态。
4. **`boot` 有缓存**：`boot(mode)` 在 `mode` 未变且非 `fresh` 时**复用现有星球**（不会重置科技/任务/背包）。需要干净状态就用 `fresh:true`，需要继续当前状态就复用。
5. **坐标约定**：工厂套件统一用 `X=40, Z=40`（远离出生点与村庄/遗迹结构），`y = topAt(X,Z)+1`。避免把机器放在结构区或被地形干扰。
6. **测试之间不残留方块/机器**：写方块/放机器的用例，结尾 `setBlock(...'air')` / `removeMachine(...)` 还原（或干脆 `fresh` 重开）。

---

## 10. 启动模式对照表

| 模式 | `creative` | `dropMult` | 初始科技 | 任务 | 生存消耗 |
|---|---|---|---|---|---|
| `creative` | true | 1 | **全解锁** | 关闭（`questIdx` 恒 0） | 无（全满） |
| `easy` | false | 7 | 仅 survival | 正常 | 有 |
| `normal` | false | 4 | 仅 survival | 正常 | 有 |
| `hard` | false | 1 | 仅 survival | 正常 | 有 |

所有生存模式 `newGame` 都固定赠送：**credits 250 · carbon 10 · sodium 5**。写「采集类任务门槛」测试时务必先 `clearInv()` 清掉赠品（见 §11.3）。

---

## 11. 已知坑与正确预期（踩坑记录，写测试前必读）

### 11.1 背包堆叠是「单格上限」不是「总量上限」

`addItem` 把物品先并入已有格（每格至 `ITEMS[x].stack`），溢出的进新格。所以：

- `give('carbon',200)` 后 `give('carbon',100)` → 总数 **300**（250 一格 + 50 一格），**不是** 250。
- `give('carbon',300)` 返回 **300**（全部入包，分两格）。
- 断言「总量上限」是错的；应断言「每格 ≤ stack」或「总数 = 250+50」。

### 11.2 `window.BLOCKS / window.ITEMS / window.TECH` 是 `undefined`

它们是 `const/let` 顶层词法绑定（§4.1）。套件里用 `api.defs.BLOCKS` 等，`test-api.js` 内部用裸名。

### 11.3 生存新游戏自带碳×10、钠×5

`newGame` 固定赠送。`tests/08-quests.js` 的「采集 15 碳」门槛测试若不清空背包，给 14 碳时总数已达 24，会直接判通过——所以那里先 `clearInv()`。

### 11.4 储物箱 / 收集点会「同类合并」

`machineInsert(chest, 'stone')` 会把同种物品累进**同一格**（直到 `ITEMS[stone].stack=250`），不会每插一次占新格。所以「插 24 个石头」= 1 格 24 个 + 23 空。要测「24 格容量」得插 **24 种不同物品**；测「满了」要插**第 25 种**不同物品（插已存在的同类会继续叠加、不算满）。

### 11.5 `Factory.group` 不是公开 API

`window.Factory.group` 为 `undefined`（`group` 是模块内部变量，未导出）。判断工厂是否初始化别用它；boot 后工厂必然已初始化（`buildPlanetScene` 调 `Factory.init`），直接 `placeMachine` 即可。

### 11.6 工厂机器需要电力才开工

- 装配机（12kW）/ 精炼厂（20kW）无电时 `sat=0`，**不生产**。测试需先放发电设备（1 块太阳能 10kW 够装配机但不够精炼厂；精炼厂用核反应堆 100kW）。
- 采矿机无电时按 `eff=max(sat,0.35)` 的 35% 效率运行（应急手摇），仍会产矿但慢（约 5.7 秒/个铁矿石）。
- 熔炉不耗电，只耗燃料（碳/煤/木板），有燃料+原料才烧。

### 11.7 `tickFactory(dt)` 的步进精度

`Factory.update` 内部按 `0.1s` 累积步进。`tickFactory(4.0)` ≈ 40 个 0.1s tick。冶炼 2.4s 配方留 4s 余量稳妥；采矿留 8~10s。

### 11.8 `research()` 会顺带推进科技类任务

`completeTech` 内部调 `checkQuest`。所以 `q_tech`（研究冶金学）在 `api.research('metallurgy')` 后**自动**推进，无需再 `pokeQuests()`。

### 11.9 同种子两次 `boot` 的世界种子一致的前提

音频已被中性化（§9.1）。若你移除了中性化、或在 boot 窗口内引入了新的 `Math.random` 定时消费者，`05-world` 的「确定性跨重启」用例会失败——那是**正确的失败**，说明确定性被破坏。

### 11.10 断言值要用 `json()` 深拷贝后的

`machineAt`/`stats`/`inv`/`power` 等已返回深拷贝；但直接读 `window.Player.stats`（live 对象）时，游戏主循环每帧都在改（氧气/防护/激光消耗），跨帧比较会抖动。测试统一用 `api.stats()` 等快照。

---

## 12. 调试与排障

### 12.1 有头模式 + 单套件

```bash
node test/run.mjs --headed --grep=factory
```

有头模式可看到游戏窗口被自动驱动；在套件文件里临时加 `console.log(api.snapshot())` 定位。

### 12.2 结果 JSON 结构

```jsonc
{
  "generatedAt": "ISO时间",
  "totalMs": 20246,
  "summary": { "suites": 10, "tests": 60, "passed": 60, "failed": 0, "ok": true },
  "suites": [
    { "name": "factory", "passed": 13, "failed": 0, "durationMs": 1234,
      "tests": [ { "name": "...", "pass": true, "error": null, "ms": 1.2 } ] }
  ],
  "pageErrors": [],
  "fatal": null            // 仅致命错误时出现
}
```

解读：`summary.ok === false` 或任何 `suites[].failed > 0` 即失败；`fatal` 非空 = 页面根本没能加载/超时。

### 12.3 常见失败与对策

| 症状 | 原因 / 对策 |
|---|---|
| `waitUntil timeout` / `fatal: ...` | 浏览器没起来或页面超时。确认装了 Edge/Chrome；看 `--headed` 下报什么；WebGL 失败检查 §12.4 |
| `game not loaded` | `window.Game` 尚未就绪；通常是 `?test` 没带上或 test-api.js 加载失败 |
| `Factory not initialized`（历史版本） | 已修：`Factory.group` 非公开；boot 后再 `placeMachine` |
| 某用例偶发失败 | 大概率是确定性被破坏或依赖墙钟；按 §9 排查 |
| `ERR_SOCKET_BAD_PORT` | `serve.mjs` 端口解析失败；默认 17899，若被占用改 `SF_TEST_PORT` 环境变量 |

### 12.4 WebGL / 浏览器

- 软渲染 WebGL 需要 `--enable-unsafe-swiftshader`（`run.mjs` 已带）。Edge 版本过老会失败 → 升级 Edge 或改用 Playwright 自带 Chromium。
- 换自带 Chromium：`npm i -D playwright && npx playwright install chromium`，然后在 `run.mjs` 的 `chromium.launch` 里去掉 `channel`（用默认下载的浏览器）。

---

## 13. CI 集成

最小集成（GitHub Actions 示例，其它 CI 同理）：

```yaml
- uses: actions/checkout@v4
- uses: actions/setup-node@v4
  with: { node-version: 20 }
- run: npm install
- run: npm test
- uses: actions/upload-artifact@v4
  if: always()
  with:
    name: test-results
    path: test-results/
```

要点：
- 依赖系统 Edge/Chrome；若 CI 镜像没有，改走 Playwright 自带 Chromium（§12.4）。
- `npm test` 非零退出即红灯；JUnit 可用 `test-results/test-results.xml` 接 Jenkins/GitLab。

---

## 14. 维护建议（持续迭代时的纪律）

1. **游戏逻辑变更后，先 `npm test`**，再读失败用例——失败信息已带 got/want，通常直接指向回归点。
2. **新增功能 = 新增套件**，一个功能至少覆盖：正常路径 + 边界（材料不足/科技门槛/容量上限）+ 持久化往返（若涉及存档）。
3. **不要改游戏只为讨好测试**；确需暴露内部状态时，优先在 `test-api.js` 用现有公开模块组合，其次才考虑给游戏加最小 hook（需同步更新 §1 的「改动边界」说明）。
4. 结果文件 `test-results/` 已被 gitignore，不要提交；`package-lock.json` 建议提交以锁定依赖版本。

---

## 附：常用速查命令

```bash
npm test                                        # 全套无头
npm run test:headed                             # 全套有头
node test/run.mjs --grep=factory                # 只跑 factory 套件（正则）
node test/run.mjs --grep=^(data|world|factory)$ # 多套件正则
node test/run.mjs --browser=chrome              # 换 Chrome
node test/run.mjs --headed --grep=quests        # 有头 + 单套件（调试首选）
$env:SF_TEST_PORT=17900; node test/run.mjs      # 换静态服务器端口（PowerShell）
```

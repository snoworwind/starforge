# STARFORGE — Technical Specification for a Rust/Bevy Port

> Repository note (2026-08-17): all legacy paths named below now live under `legacy-web/` (for example, `js/main.js` is `legacy-web/js/main.js`).

Source files analyzed: `js/savestore.js`, `js/ui.js`, `js/main.js` (game-state machine / save / key-bindings / main-loop portions), plus `js/player.js`, `js/factory.js`, `js/world.js`, `js/creatures.js`, `js/data.js`, `index.html`, `css/style.css`.

> Version markers observed in source: `build v186`, `window.__V_MAIN = 'v187'`; save format is **v4** (older v≤3 saves are incompatible and are exported/deleted, not migrated).

---

## 1. Save system

### 1.1 Storage backend

The game does **not** use `localStorage` for saves anymore. It uses **IndexedDB** via the `SaveStore` singleton (`js/savestore.js`).

- Database name: **`starforge`**, version **2**.
- Object store **`saves`** — holds all slot/character/world/index records (plain JS objects; structured-clone serialized).
- Object store **`chunks`** — holds full chunk snapshots (Minecraft-style).
- If IndexedDB fails to open (`onerror`/`onblocked`/exception), the store degrades to an **in-memory `Map`** (session-only). `SaveStore.available` is `opened && mem === null`.
- Writes are atomic: slot + index are written in one `readwrite` transaction (`atomicWrite`). Chunk batches are written in one transaction (`putChunks`).
- All `req()` promises resolve `null`/`false` on error — they never reject.

**`localStorage` keys that still exist (not used for saves):**

| Key | Purpose |
|---|---|
| `starforge_settings` | Graphics/game settings JSON (persisted) |
| `starforge_index` | **Legacy** slot index (read once during migration) |
| `starforge_save1` | **Legacy** single-save record (read once during migration) |

**Reserved keys inside the `saves` store:**

| Key | Value |
|---|---|
| `__index` | Array of **session-slot** entries (the "档案" pairing records) |
| `__migrated` | `1` when the legacy `localStorage` migration completed |

**Record keys (in `saves` store):**

| Prefix | Record kind | Index key |
|---|---|---|
| `starforge_ch_<id>` | character record | `starforge_chars_index` |
| `starforge_wd_<id>` | world record | `starforge_worlds_index` |
| `starforge_sv_<id>` | session-slot pairing record | `__index` |

**Chunk store key format** (in `chunks` store, prefixed by `chunks/`):

```
chunks/<worldKey>|<galaxySeed>|<pid>|<cx>,<cz>
```

record value: `{ v: 1, mod: 0|1, data: Uint16Array }` where `data` is RLE `[run, blockId]` pairs.

`newId(prefix)` = `prefix + Date.now().toString(36) + '_' + random(6 base36 chars)`.

### 1.2 Character record — exact JSON schema

Produced by `buildCharData()` (main.js) + `Player.serialize()` (player.js). Stored at `starforge_ch_<id>`.

```json
{
  "v": 4,
  "kind": "char",
  "name": "旅行者",
  "appearance": {
    "skin": "#e8c49a",
    "hairStyle": "short",
    "hair": "#4a3018",
    "suit": "#4a5a6e",
    "trim": "#35e0e8",
    "pants": "#33404c",
    "boots": "#1e262e",
    "helmet": true,
    "visor": "#ffb347"
  },
  "player": {
    "pos": [96.0, 40.0, 96.0],
    "yaw": 0.0,
    "pitch": 0.0,
    "stats": {
      "hp": 8, "hpMax": 8,
      "shield": 6, "shieldMax": 6,
      "o2": 100, "o2Max": 100,
      "haz": 100, "hazMax": 100,
      "jet": 100, "jetMax": 100,
      "laser": 100, "laserMax": 100
    },
    "inv": [
      { "item": "carbon", "n": 10 },
      { "item": "sodium", "n": 5 },
      null,
      "... 33 more entries → total 36 ..."
    ],
    "hotIdx": -1,
    "credits": 250,
    "appearance": { "..." : "duplicate of top-level appearance" }
  },
  "techState": { "survival": true, "metallurgy": true, "...": true },
  "questIdx": 0,
  "playTime": 0.0,
  "fuelLoaded": 0,
  "playerShip": { "model": "ship", "cls": "C", "name": "拓荒者号", "inv": [null, null, null, null, null, null, null, null, null, null, null, null] },
  "shipGarage": [],
  "researching": null
}
```

Field notes:

- `inv`: **36 elements** (index 0–8 = hotbar, 9–35 = storage). Each element is `null` or `{ "item": <itemId string>, "n": <count int> }`. Stacks are capped by `ITEMS[id].stack` (default 250).
- `hotIdx`: `-1` means the fixed "mining laser" slot is selected; `0..8` = hotbar slot.
- `stats`: hp/hpMax/shield/shieldMax are **integer "segments"**; o2/haz/jet/laser are **0–100 floats**.
- `appearance` appears twice (top-level and inside `player`) — both written; `applyCharData` reads `c.appearance || p.appearance || null`.
- `techState`: object mapping tech-id → `true`. Default `{ "survival": true }` (survival tech is always unlocked).
- `playerShip`: `{ model, cls, name, inv: [12 nulls] }` — 12-slot ship cargo. `shipGarage` is an array of ship objects (same shape), empty by default.
- `researching`: `{ "id": <techId>, "t": <elapsedSeconds float> }` or `null`. In-flight timed research (cost already paid) is persisted.
- `fuelLoaded`: ship fuel count (0/1 for takeoff). `playTime`: cumulative seconds. `questIdx`: quest progress pointer. `credits`: integer ₪.

**New-character defaults** (`createCharacter`, main.js): `player.pos = [0,40,0]`, stats as above, `inv` = carbon×10 + sodium×5 (36-slot array), `hotIdx: -1`, `credits: 250`, `fuelLoaded: 0`, `playerShip = {model:'ship', cls:'C', name:'拓荒者号', inv: 12×null}`, `shipGarage: []`, `techState: {survival:true}`, `questIdx: 0`, `playTime: 0`.

### 1.3 World record — exact JSON schema

Produced by `buildWorldData()` (main.js). Stored at `starforge_wd_<id>`.

```json
{
  "v": 4,
  "kind": "world",
  "name": "未命名世界",
  "terrainV": 1,
  "state": "space",
  "currentPlanet": 0,
  "dayTime": 0.3,
  "flags": { "checkedShip": true, "sideQuest": null, "...": true },
  "market": { "carbon": 1.05, "oxygen": 0.92, "...": 1.0 },
  "placedCount": { "furnace": 1 },
  "creative": false,
  "dropMult": 4,
  "galaxySeed": 7777,
  "galaxyCount": 1,
  "planets": {
    "0": {
      "machines": [
        { "x": 4, "y": 40, "z": 3, "type": "furnace", "dir": 0, "data": { "in": { "item": "iron_ore", "n": 1 }, "fuel": null, "out": null, "prog": 0.0, "burn": 0.0 } }
      ],
      "shipPos": [100.0, 41.0, 98.0],
      "seed": 4294967295,
      "biome": "lush",
      "creatures": {
        "herds": [ [cx, cz, candIdx, x*10, z*10, hp, homeX*10, homeZ*10], "...8 ints per herd..." ],
        "removed": [ ["12,34", 7] ]
      }
    }
  },
  "galaxyArchives": {
    "4321": {
      "1": { "machines": [], "shipPos": [0,0,0], "seed": 999, "biome": "desert", "creatures": null }
    },
    "4321_marks": {
      "1": [ { "x": 10, "z": 20, "y": 41, "label": "标记", "gal": false } ]
    }
  },
  "mapMarks": {
    "0": [ { "x": 10, "z": 20, "y": 41, "label": "标记", "gal": false } ]
  },
  "playerPlace": { "pos": [96.0, 40.0, 96.0], "yaw": 0.0, "pitch": 0.0 },
  "warpLock": { "seed": 4321, "name": "天琴-α" },
  "shipState": { "pos": [100.0, 50.0, 98.0], "yaw": 0.1, "pitch": 0.05 }
}
```

Field notes:

- `terrainV`: terrain generator version (`1`), reserved for future upgrades / net consistency.
- `state`: the current game state, but the station-phase names (`station`, `docked`, `dockAnim`, `stationed`, `stationWalk`, `undockAnim`) are all collapsed to **`"space"`**. Otherwise it is `"planet"` (or `"space"`).
- `currentPlanet`: index into the current galaxy's `SYSTEM_PLANETS`.
- `dayTime`: 0–1 fractional day.
- `flags`: quest event flags object (keys e.g. `checkedShip`, `shipRepaired`, `launched`, `docked`, `traded`, `newPlanet`, `warpedOut`, and `sideQuest` = village side-quest `{item, need, reward, from, x, z}` or null).
- `market`: `{ itemId: priceMultiplier }`. Home galaxy init `0.9 + rand*0.2` (0.9–1.1); generated galaxies `0.75 + rand*0.5` (0.75–1.25). Trade uses `buy = round(price*mod*1.25*discount)`, `sell = round(price*mod*0.8)`; discount = 0.85 with `trade_ai` tech.
- `placedCount`: `{ blockKey: count }` — placement-progress for the "place N of X" quests.
- `creative` (bool), `dropMult` (1=hard, 4=normal, 7=easy; 1 in creative).
- `galaxySeed`: current galaxy seed; `galaxyCount`: number of galaxies visited/created.
- `planets`: map `pid → planet-world-state`. **v4 omits `mods`** (modified-chunk RLE now lives in the `chunks` store). Each entry: `machines` (Factory.serialize array), `shipPos` `[x,y,z]`, `seed` (world noise seed), `biome` (string key), `creatures` (`Creatures.serialize()` result or null).
- `galaxyArchives`: map `galaxySeed → planets` plus `galaxySeed + "_marks" → mapMarks`. Cross-galaxy beacons/buildings/markers persist this way. Entries are `stripPlanetsMods`-ed (no `mods`).
- `mapMarks`: current galaxy's `pid → [ {x, z, y, label, gal} ]`. `gal: true` = shown across the whole galaxy (can be a landing target from space); `false` = planet-only.
- `playerPlace`: last player position/orientation; used as spawn on load.
- `warpLock`: `{ seed, name }` or `null`.
- `shipState`: `{ pos: [x,y,z], yaw, pitch }` — **only present when `state !== 'planet'`** (else `null`). If saved inside a station, `pos` is the dock exit point.

### 1.4 Session-slot (pairing) record — exact schema

Stored at `starforge_sv_<id>`; also mirrored as the entry inside `__index`.

```json
{
  "v": 4,
  "kind": "session",
  "key": "starforge_sv_abc123_xyz",
  "name": "档案 1",
  "time": 1710000000000,
  "charKey": "starforge_ch_...",
  "worldKey": "starforge_wd_...",
  "charName": "旅行者",
  "worldName": "未命名世界",
  "creative": false,
  "planetName": "始源星",
  "credits": 250,
  "playMin": 12
}
```

### 1.5 Import / export

**Export** (`exportSave`, main.js):

- Fetches the slot. If it is a `session`, bundles it as:

```json
{
  "v": 4,
  "kind": "bundle",
  "session": { "...session record..." },
  "char": { "...character record..." },
  "world": { "...world record..." },
  "chunks": { "<relativeKey>": [1, [9, 1, 3, 5, ...]] }
}
```

where `<relativeKey>` = chunk key with the `worldKey + '|'` prefix stripped (i.e. `galaxySeed|pid|cx,cz`), and the value is `[ modBit(0|1), RLE data as a plain array ]`.

- Filename: `STARFORGE-档案-<name>-<ISO date>.json` (name sanitized: `/[\\/:*?"<>|]/g → '_'`, max 40 chars). Pretty-printed with 2-space indent. Blob `application/json`. A non-session slot (e.g. legacy) is exported as the raw record via `exportSlotJson` (filename = key sanitized + `.json`).

**Import** (`importSave`, main.js):

- Max file size **50 MB**.
- Accepts three shapes: `kind:'bundle'` (char+world+chunks), `kind:'char'`, `kind:'world'`. All must be **v4** or it is rejected with "版本过旧".
- Re-keys everything: new `starforge_ch_<id>` / `starforge_wd_<id>` / `starforge_sv_<id>`; chunk relative keys remapped to the new worldKey. If both char+world, a session slot is created.

### 1.6 Autosave & save timing

- **Autosave**: `setInterval(…, 60000)` → `save()` **only if** `state === 'planet' && !worldPaused() && activeSaveKey`.
- **Manual**: `F5` quick-save; Esc menu → 存档; Pause → 存档 (`UI.openSavePanel('save')`); `btnQuit` auto-saves before reload (if `activeSaveKey` and not a net guest).
- **saveTo guards**: refuses when `state` is `menu`/`loading` (returns false), and refuses with a message in `atmo`/`atmoland`/`seated`/`launching`/`warping`. Net **guest** role is saved by the server (local save blocked).
- **Write order** (saveTo): snapshot char+world → `flushAllChunks()` (chunk snapshots first, Minecraft-style) → `putChar` → `putWorld` → `putSlot`/`atomicWrite` session slot + `__index`.
- **Chunk streaming**: `flushChunkQueue(6)` runs every frame (6 modified chunks per frame batch, background, `chunkWriteBusy` guarded); `persistModsToStore` writes all `mods` RLE for a planet at once. Chunk records use `{ v:1, mod:1, data: Uint16Array }`.

---

## 2. UI — Inventory & Crafting

### 2.1 Inventory layout

Panel `#invPanel` (title "◈ 外骨骼背包"). Body split `inv-left` / `inv-right`.

- **Hotbar row** `#invHotRow`: 9 slots (`inv[0..8]`), CSS grid `repeat(9, 1fr)`.
- **Storage grid** `#invGrid`: 27 slots (`inv[9..35]`), same 9-column grid → 3 rows of 9.
- Total **36 slots**. Slot class `.slot`; filled slot shows `Icons.img(item)` + `.cnt` count (hidden when `n === 1`).
- **Charge panel** `#chargeList` (survival only; hidden entirely in creative). One row per `Player.CHARGE_DEFS` entry: icon, name, fill bar, and a button `充能 <item>×<cost>`. Definitions:

| kind | name | item | cost | gain | stat |
|---|---|---|---|---|---|
| laser | 采矿激光 | carbon | 3 | 30 | laser |
| shield | 偏导护盾 | sodium | 2 | 2 | shield |
| hp | 生命系统 | oxygen | 4 | 2 | hp |
| o2 | 生命维持 | oxygen | 1 | 30 | o2 |
| haz | 危险防护 | sodium | 1 | 25 | haz |

- Bottom bar `#invBottom`: `#trashSlot` (🗑 recycle), `#invSortBtn` (🧹 整理), `#invHint` (controls legend: "左键：选取/放下 · 右键：拆半 · Shift+左键：快速移动 · G：丢出 · 🗑：销毁手持 · 🧹：整理").

### 2.2 Crafting list (right column)

Title "⚒ 便携合成". Category tabs `#craftTabs`:

| key | label |
|---|---|
| `all` | 全部 |
| `mat` | 材料 |
| `mach` | 机器 |
| `blk` | 方块 |

- `craftCat` defaults to `all`. Category filters by **output item's** `ITEMS[out].cat` (res/mat/blk/mach).
- Recipe visibility: only `r.where === 'hand' || r.where === 'both'`, and `!r.hidden` are shown. (No recipe actually uses `where:'hand'`; portable recipes all use `where:'both'`.)
- Each recipe row `#craftList .recipe`: icon + name (`name ×N` if out > 1) + cost line (`.rcost`) + "合成" button. Hover shows tooltip; if tech-locked, appends `需要科技：<TECH name>` in `#ff5555`.
- `refreshInv` updates cost text: tech-locked → `🔒 <TECH name>`; otherwise each input as `name×n` with class `ok` (green, have enough) or `no` (red). Locked/unavailable recipe gets class `.locked` (opacity 0.4, grayscale, not-allowed).
- **Click** = craft 1; **Shift+Click** = craft 5 (loops `tryCraft` until fail). Output is multiplied by `Game.dropMult`; overflow drops at the player (`spawnDrop` at `pos.x, pos.y+0.6, pos.z`).

### 2.3 Click / drag / right-click semantics (inventory slots)

`bindSlotEvents` (ui.js):

- **Left-click (button 0)**:
  - cursor empty + slot full → pick up whole stack (clone).
  - cursor full + slot empty → place entire cursor stack.
  - both full, same item → merge up to `ITEMS[item].stack`; leftover stays on cursor.
  - both full, different items → swap.
- **Right-click (button 2)**:
  - cursor empty + slot full → pick up **half, `ceil(n/2)`** (split); if `n===1`, whole stack.
  - cursor full + slot empty → place **1**.
  - cursor full + slot same item (and slot not full) → add **1**.
- **Shift+Left-click**: quick-move between hotbar and inventory. For slot index `< 9` (hotbar), target = storage `9..35`; for `>= 9`, target = hotbar `0..8`. Merges into existing partial stacks first, then into first empty slot.
- **Right-click** (contextmenu) is `preventDefault`ed everywhere (no native menu).
- Cursor stack (`cursorStack`) is rendered by `#dragGhost` (44×44, follows mouse at `clientX-22, clientY-22`; count shown when `n>1`).
- On any panel close or cursor drop, `dropCursor()` returns the cursor stack to inventory; overflow spawns as world drops.

### 2.4 Sort function — exact behavior

`Player.sortInventory()` (player.js):

1. Only operates on **slots 9–35** (storage). Hotbar 0–8 untouched.
2. Collects totals per item in first-encounter order (a `Map` + order array).
3. Rebuilds an output array by emitting full stacks of `ITEMS[item].stack` (default 250) until each total is exhausted.
4. Writes output back into slots 9–35, `null`-filling the rest (empty slots sink to the end).

### 2.5 Crafting station tabs (machine panel)

Machine panel `#machinePanel` body is rebuilt per machine type (`buildMachineBody`). Crafting-type machines:

- **Furnace** (熔炉): `in` (accept filter = recipes where `where==='furnace'` containing that input), `fuel` (accept filter = `FUEL_VALUE[item]` truthy), `out`; progress bar; stat `燃烧余量 Xs · 燃料：碳(4s) 煤(16s)`. (`FUEL_VALUE = { carbon: 4, coal: 16, planks_b: 5 }`.)
- **Assembler** (装配机): recipe picker shows recipes where `where ∈ ['both','assembler']` and tech-unlocked; input slots per recipe `d.in[k]` (accept filter `it === k`); output; progress; `耗电 12kW`.
- **Refinery** (精炼厂): recipe picker shows `where === 'refinery'` (tech-unlocked); same layout; `耗电 20kW`.
- Switching/clearing a recipe refunds slot materials and (if progress > 0) one set of `r.in` (overflow drops at `m.x+0.5, m.y+1.2, m.z+0.5`).
- **Chest / Collector**: a `slot-grid` of `d.slots` (chest = 24 slots per its item description; collector = 12). Shift+click pulls whole stack to inventory.
- **Reactor**: `投料铀-235（+60s）` button, fuel display, `输出 100kW`.
- **Burner**: fuel slot (coal/carbon), progress, `25kW`.
- Every non-static machine panel also embeds the player's 36-slot **exoskeleton inventory** (Shift+click tries to push into the machine via `Factory.canMachineAccept`/`machineInsert`; belts break after 1 item).

`tickMachinePanel(dt)` rebuilds the DOM only every **0.4 s** and only if `machSignature()` changed (per-type state signature), to avoid interrupting drag/click. Beacon panel never auto-rebuilds (input focus).

---

## 3. HUD — every element

All colors come from CSS variables: `--cyan #35e0e8`, `--cyan-d #0e6d78`, `--amber #ffb347`, `--red #ff5555`, `--green #7dff8a`, `--gold #ffd166`, `--purple #b48cff`, `--txt #c9e6ee`, `--dim #7f9db0`, `--grid #1d3a52`, `--bg #05070d`. Font: `'Segoe UI'`, mono: `'Consolas'`. HUD container `#hud` is `position:fixed; inset:0; z-index:40; pointer-events:none`.

### 3.1 Crosshair — `#crosshair`

- Centered (`top/left 50%`, translate −50%/−50%), **22×22 px**.
- Vertical bar 2px wide, horizontal bar 2px tall, color `#eafcffcc` (with `box-shadow 0 0 4px #35e0e8`).
- Center ring: `inset:6px`, 1px `#35e0e888` border-radius 50% (10px diameter circle).

### 3.2 Vitals (top-left) — `#vitals`

- Position `top:16px; left:16px; width:230px`.
- Row 1 **Shield** (`#barShield`): label `护盾`, segmented bar `.segbar` — height **12px**, flex segments, 2px gap; segment `i` background `#12324a`; active `.on` gradient `#35e0e8 → #8ff` with glow. Number of segments = `shieldMax` (6).
- Row 2 **HP** (`#barHP`): `.segbar.hp`, active gradient `#ff6b6b → #ffb3b3`, glow `#ff5555`. Segments = `hpMax` (8).
- Row 3 **Oxygen** (`#barO2`): thin bar height **6px**, fill gradient `#5bc0ff → #c9f0ff`.
- Row 4 **Hazard protection** (`#barHaz`): thin bar, fill gradient `#ffb347 → #ffe0a0`, plus `#hazIcon` (⚠ indicator when hazard active).
- Row 5 **Laser** (`#barLaser`): thin bar, fill gradient `#ff6a4d → #ffc2b3`.
- `#envInfo` below (biome/time text, `--dim`, left border 2px `--cyan-d`).
- Labels `.vlabel`: 10px, letter-spacing 2px, width 32px, `--dim`.

### 3.3 Quests (top-right) — `#quests`

- Position `top:16px; right:16px; width:250px`; background `#0a1420cc`; border 1px `--cyan-d`, right border 3px `--cyan`; clip-path chamfered corner.
- `.q-head` "◈ 任务日志" (cyan, 11px). `.q-item` entries (12px); done = green strikethrough; side quests = gold; `.qp` progress = amber mono. `#questTip` guide line (dashed top border).

### 3.4 Hotbar (bottom-center) — `#hotbarWrap`

- `#hotbarWrap`: `bottom:16px; left:50%; translateX(-50%)`, column flex, 6px gap.
- `#jetpackBar`: **280×4 px**, `#12324a` bg; `#jetFill` gradient `#ffb347 → #ffe0a0`; only shown (`opacity 0→1`, class `.show`) when `jet < jetMax-1`.
- `#itemLabel`: name label (Minecraft-style), shows selected item name for 900 ms (cyan glow text).
- `#hotbar`: row of **10 slots** — 9 item slots (`0..8`, key labels 1–9) + 1 fixed **mining laser** slot (key label `0`). Slot `.hslot` = **54×54 px**, bg `#0a1420cc`, border 1px `#24405a`; hover `--cyan-d`; selected `.sel` amber border + glow + `translateY(-4px)`. Laser slot special: border `#c9641a55`, bg `#160f0acc`; icon is a 32×32 canvas drawing (body `#4e5a63`, top `#68747d`, barrel `#333d44`, muzzle ring `#c9641a`, energy screen `#35e0e8`, tail `#c9641a`, grip `#333d44`).
- `.hslot .num` = slot number (10px `--dim`, top-left). `.hslot .cnt` = stack count (12px white bold, bottom-right).
- `#powerInfo`: positioned `right:-140px; bottom:0`, amber mono 12px `⚡ <gen>/<use> kW`; text color `#ff5555` when power satisfaction < 1, else `#ffb347`.

### 3.5 Space HUD — `#spaceHud` (hidden on ground)

- `#speedo`: `bottom:40px; left:50%`; label 10px `--dim` "速度"; `#speedVal` 38px mono cyan glow; unit "u/s" 11px.
- `#pulseHint`: `bottom:40px; right:60px`, amber 13px mono (pulse-charge readout).
- `#targetInfo`: `top:70px; left:50%`, cyan, bordered, bg `#0a142088`.
- `#tritiumInfo`: `bottom:40px; left:60px`, cyan mono 14px `◇ 氚 <n>`.

### 3.6 Misc HUD

- `#creditHud`: `top:14px; left:50%` center; gold mono 16px `₪ <credits>`, bg `#0a142088`, border 1px `#ffd16644`.
- `#clockHud`: `bottom:16px; right:16px`, `--dim` mono 12px (local time).
- `#markers` (world markers): absolute inset 0, `.wmark` floating labels (ship = cyan, beacon = gold, ore = green), pulsing icons, edge-clamped.
- `#pickups` (pickup toasts): `bottom:130px; left:20px`, column-reverse; each `.pickup` = icon + name + gold `+n`; merges same item within 2.6 s; max 5 entries.
- `#interactHint`: `bottom:150px; left:50%`; bg `#0a1420dd`, border `--cyan-d`, 14px; key caps are `b` chips (cyan bg, dark text).
- `#bigMsg`: `top:32%` center; 30px cyan glowing title, amber 15px subtitle; default 3.2 s (used for `bigMessage`).
- `#tooltip`: fixed z-200, bg `#060d16f0`, border `--cyan-d`, max-width 240px; `.tt-name` cyan 13px, `.tt-cat` amber 10px uppercase, `.tt-desc` dim italic. Positioned at cursor + (16, 12), clamped to viewport (width−260, height−120).
- `#vignette` (inset shadow), `#damageFlash` (radial red on `.hit`), `#reentryFx` (orange radial re-entry glow), `#atmoTint` (atmosphere color filter), `#fader` (black fade), `#dialogBox` (RPG dialog, bottom 14vh).

---

## 4. Main menu flow

Boot screen `#boot` (`z-index:100`), inner `#bootInner`. Title "STARFORGE / 星 穹 熔 炉", subtitle "体素星球 · 星际贸易 · 自动化工厂", version line "v1.6 · 单机 + 联机 · 像素风体素星际工厂". Tips rotate every 4 s while in `menu` state.

### 4.1 Title screen options (`#bootMenu`)

| Button | Action |
|---|---|
| ▶ 开始新纪元 (`btnNew`) | → `#modeSelect` |
| ↻ 继续档案 (`btnContinue`) | → `UI.openSavePanel('load')` (disabled until a save exists) |
| ⇄ 联机 (`btnNetBoot`) | → net panel |
| ⚙ 画面设置 (`btnSettingsBoot`) | → `#settingsPanel` |
| ? 操作指南 (`btnHelp`) | → `#helpPanel` |

### 4.2 Mode select (`#modeSelect`)

- ⛏ 生存模式 (`btnSurvival`) → `#diffSelect`.
- ✦ 创造模式 (`btnCreative`) → `UI.openCharCreate(true)`.
- ← 返回 (`btnModeBack`).

### 4.3 Difficulty select (`#diffSelect`)

| Button | mult |
|---|---|
| ☘ 简单模式 (×7) `btnDiffEasy` | 7 |
| ◈ 普通模式 (×4) `btnDiffNormal` | 4 |
| ☠ 困难模式 (×1) `btnDiffHard` | 1 |

Each → `UI.openCharCreate(false, mult)`.

### 4.4 Character creation (`#charCreate`) — 9 appearance parts

`openCharCreate(mode, mult)` builds `cc.app` (defaults from `randomAppearance()`). Fields (swatch groups → `cc.app` key):

| UI label | key | Option list | Default (random) |
|---|---|---|---|
| 肤色 | `skin` | `#e8c49a, #d8b48a, #c89878, #8d5a3c, #6b4630, #f0d8b8, #b98e6a, #e8d0b0` | any |
| 发型 | `hairStyle` | `none/无, short/短发, long/长发, pony/马尾, mohawk/莫霍克, bun/发髻` | any of the 5 non-none |
| 发色 | `hair` | `#4a3018, #2e2620, #5a4632, #7a5a8a, #a86a3a, #d8c8a8, #c23a3a, #1e2e4a` | any |
| 制服 | `suit` | `#4a5a6e, #3fa8c9, #5a3e3e, #6e6a2a, #3e5a6e, #4a4258, #5a6a3a, #7a3a2a` | any |
| 饰条 | `trim` | `#35e0e8, #ffb347, #ff6a5e, #b58aff, #7dff8a, #ffd94d, #f0f0f0, #35b0ff` | any |
| 裤装 | `pants` | `#33404c, #4a3c2e, #2e3a44, #3a3248, #3e3a2e, #443430` | any |
| 靴子 | `boots` | `#1e262e, #2e2620, #26221a, #241e2e, #2a221e, #33261a` | any |
| 头盔 | `helmet` | `true/头盔开, false/头盔关` | 70% true |
| 目镜 | `visor` | `#ffb347, #35e0e8, #ff6a5e, #b58aff, #7dff8a, #f0f0f0` | any |

- Name input `#charNameInput` (maxlength 10, default placeholder "旅行者").
- 3D live preview (`#charPrevCanvas` 300×360) with rotating camera + walk animation (Humanoid).
- Buttons: 🎲 随机外观 (`btnCharRandom`), ✓ 下一步：创建世界 (`btnCharConfirm`), ← 返回 (`btnCharBack`).
- Confirm: name `= trim || '旅行者'`; then → `#worldCreate` (char + world flow). In save-panel mode (`fromSavePanel`), it instead persists the character and returns to the pair picker.

### 4.5 World creation (`#worldCreate`)

- 世界名 `#worldNameInput` (maxlength 16, placeholder "新世界").
- 种子 `#worldSeedInput` (maxlength 12, placeholder "留空 = 随机"). Parsed with `parseInt(raw,10)`; invalid/empty → undefined → random seed.
- 难度 buttons: ☘ 简单 ×7 / ◈ 普通 ×4 (default `on`) / ☠ 困难 ×1 / ✦ 创造. Selection drives `wcState { creative, mult }` — **this** page's selection is authoritative for the final game.
- Buttons: ✓ 出发！ (`btnWorldConfirm`), ← 返回 (`btnWorldBack`).
- Confirm → `Game.newGame(creative, mult, { char, world: { name, seed } })`.

### 4.6 Settings — exact option lists & defaults

Persisted to `localStorage['starforge_settings']`. Defaults (main.js):

```js
{ fov: 75, chunkDist: 16, farDist: 1536, quality: 'mid', planetLod: 'mid',
  clouds: 'on', realAtmo: 'on', npcShips: 7, mouseSens: 1, style: 'modern', weather: 'on' }
```

| Setting | Control | Range / options | Default |
|---|---|---|---|
| 视野 FOV | `#setFov` slider | 60–100, step 1 | 75 |
| 鼠标灵敏度 | `#setMouseSens` slider | 0.1–3.0, step 0.05 | 1.0 |
| 区块渲染距离 | `#setChunk` slider | 6–33, step 1 (33 = ∞) | 16 |
| 星球区块 | `#setPlanetLod` buttons | low 低 / mid 标准 / high 高 / ultra 极高 | mid |
| 可视距离 | `#setFar` slider | 400–1536, step 64 | 1536 |
| 画质 | `#setQuality` buttons | low 流畅 / mid 标准 / high 高画质 | mid |
| 体积云 | `#setClouds` buttons | on 开启 / off 关闭 | on |
| NPC 飞船数量 | `#setNpc` slider | 0–20, step 1 | 7 |
| 渲染风格 | `#setStyle` buttons | modern 现代 / pixel 像素 | modern |
| 天气 | `#setWeather` buttons | on / off | on |
| 逼真大气层 | `#setRealAtmo` buttons | on / off | on |

Volume (pause panel) `#volSlider`: 0–100, default **70**.

---

## 5. Tech tree UI (key T)

Panel `#techPanel` (class `wide`), title "◈ 科技矩阵", header shows `#dataCount` = `⬡ 研究数据 ×<count of 'data'>`. Canvas area `#techCanvasWrap` contains `#techLines` (SVG) + `#techNodes` (absolute 1400×900 layer).

- **Currency**: item `data` (研究数据). Each tech has `cost {item:n}`; `survival` is free (`cost:{}`, `time:0`, `unlocked:true`).
- **Nodes**: `.tnode` 118px wide, positioned at `t.pos[0], t.pos[1]` px. States: `.done` (green border/bg), `.avail` (amber, requirements met), `.locked` (opacity 0.5, grayscale 0.6). Node shows icon (38×38), name (11px), cost (10px amber mono) or "✔ 已解锁" / "研究中 X%".
- **Edges**: SVG lines from each `req` node center (+59,+45) to node center; stroke `#7dff8a66` (done), `#ffb34766` (req done), `#24405a` (locked); dasharray `6 4` unless done.
- **Unlock interaction**: click a node with all `req` done and no research in progress → pays cost, starts `researching = {id, t:0}`. If materials missing → "材料不足" big message.
- **Research progression**: `updateResearch(dt)` adds `dt` to `researching.t`; on `t >= t.time` → `Game.completeTech(id)`, clears researching, plays "research", shows "科技解锁 <name> — <desc>". Only **one** research at a time. Progress persists in the character save.

Tech table (id, name, cost, time, pos `[x,y]`, req):

| id | name | cost | time (s) | pos | req |
|---|---|---|---|---|---|
| survival | 生存本能 | {} | 0 | [60, 380] | [] |
| scan1 | 扫描增幅 I | data×4 | 10 | [230, 200] | survival |
| scan2 | 扫描增幅 II | data×15, circuit×4 | 20 | [400, 120] | scan1 |
| metallurgy | 冶金学 | data×2 | 8 | [230, 380] | survival |
| automation | 自动化 | data×5 | 15 | [400, 260] | metallurgy |
| logistics | 物流学 | data×4 | 12 | [400, 500] | metallurgy |
| power | 清洁能源 | data×8 | 20 | [570, 260] | automation |
| assembly | 装配流水线 | data×12 | 25 | [570, 440] | automation, logistics |
| refining | 化学精炼 | data×15 | 30 | [740, 340] | power, assembly |
| spaceport | 航天工程 | data×20, titanium×10 | 35 | [910, 260] | refining |
| nuclear | 核裂变 | data×30, uranium×5 | 45 | [910, 440] | refining |
| trade_ai | 贸易协议 | data×18, gold×3 | 25 | [1080, 340] | spaceport |
| warp | 曲率理论 | data×60, tritium×50 | 60 | [1250, 340] | trade_ai, nuclear |

---

## 6. Map UI (key M — planet map)

`togglePlanetMap()` toggles `#mapPanel` (class `wide`, title "◈ 星球全息地图", header `#mapInfo` = `X … · Z … · 标记 N 个`).

- **View** `#mapView`: `#mapCanvas` 640×470 (WebGL-rendered), `#mapTip` hover tooltip.
- **Sphere**: radius `MAP_R = 100`, Lambert material textured with a 512×256 procedural terrain canvas (redrawn row-by-row, 6 rows/frame, using `World.mapColorRGB`); wireframe overlay (cyan, opacity 0.06). Camera FOV 45, position z=262. Ambient 0xffffff×0.22 + directional sun synced to world day/night.
- **Controls**: drag = rotate (yaw += dx·0.007, pitch clamped ±1.4); click (moved<5px) = raycast the sphere → `mapPending = {x, z}`; opens `#mapAddForm` with selected coords + local time; type name (maxlength 12, Enter confirms) and choose scope (⚑ 仅本星球 / ✦ 全星系显示). Wheel not used here.
- **Sidebar** `#mapSide`: add-form, existing-marks list (`#mapMarkList`, toggle scope / delete), legend (我 `#7dff8a`, 飞船 `#35e0e8`, 信标方块 `#ffa030`, 标记 `#ffd94d`, 全星系标记 `#c07dff`).
- **Pins** (`refreshMapMarks3d`): map marks (`#ffd94d` normal / `#c07dff` galaxy-wide), beacons `#ffa030`, POIs (village `#4dc86a`, ruin `#d8b038`), pending `#ff4444`, ship `#35e0e8`, player arrow (`#7dff8a` on ground / `#35e0e8` in ship, white body + glow, oriented to yaw, pulsing).
- `mapMarks` storage: `pid → [{x, z, y, label, gal}]`. `y = World.topAt(x,z)+1`.
- Open while in `planet`/`seated`/`atmo`; in `space` the M key opens the **galaxy map** instead (`#galaxyPanel`, full-screen 3D starfield with spectral star classes G/M/E/B, filter buttons 自由探索/黄星/红星/绿星/蓝星/已到访, hover detection, click-to-select detail card, warp-route lock with `◎ 锁定星系`).

---

## 7. Game states, transitions, main loop, key bindings

### 7.1 State set (actual assignments)

Declared `state = 'menu'`. States actually assigned in the current code:

| State | Meaning |
|---|---|
| `menu` | Boot/title screen |
| `loading` | Planet generation (loading screen) |
| `planet` | On-foot on a planet |
| `seated` | Seated in landed ship (boarded, not yet flying) |
| `atmo` | Flying in atmosphere |
| `atmoland` | Landing animation (air → ground) |
| `space` | In space (flying ship) |
| `warping` | Warp jump animation |
| `station` | Docked/walking inside space station |

Note: the save guard and a station-state list also reference `launching`, `docked`, `dockAnim`, `stationed`, `stationWalk`, `undockAnim`, but these are **legacy/defensive names** — the only live station state is `station`, and `launching` is never assigned. `worldPaused()` freezes simulation only for the system panels (`pausePanel`, `settingsPanel`, `helpPanel`, `savePanel`) and only in single-player.

### 7.2 Transitions & init flow

- **Boot → new game**: `btnNew → modeSelect → (diffSelect | creative) → charCreate → worldCreate → Game.newGame(creative, mult, {char, world})`.
- `newGame()`: sets flags/credits/inventory/starter ship, `Space.restoreGalaxy(HOME_GALAXY_SEED=7777)`, then `genPlanet(0, fresh, ...)` → `state='loading'` → build world → write char+world records → `state='planet'` → `lockPointer()`.
- `genPlanet(pid, …)`: `state='loading'`, shows `#loading`, `World.pregen` (progress bar, 9 load-flavor strings), `buildPlanetScene()`, deserializes machines/creatures/ship, hides loading, `state='planet'` (unless deferred).
- **Load**: `loadPair(charKey, worldKey)` (creates a session slot if missing) → `loadFrom(key)` → validate `v4`/kinds → `applyCharData` → `applyWorldData` → `genPlanet(currentPlanet, false, …)`. If `wd.state==='space'` and `wd.shipState` exists → `Space.enter`, restore ship pose, `state='space'`.
- **Board ship (E near ship)**: `boardShip()` → `state='seated'` (hides player, idle engine). `exitShip()` → `state='planet'`.
- **Takeoff (W while seated)**: `attemptTakeoff()` — needs `fuelLoaded≥1` (consumes 1 `fuel` from inventory) or a launchpad underneath, else `launch()` → `startAtmo(false)` → `state='atmo'`.
- **Atmosphere → space**: when the ship climbs out of the atmosphere → `finishLaunch()` → `Space.enter` + coordinate remap → `state='space'`.
- **Space → atmosphere (seamless re-entry)**: `seamlessApproach()` → `startAtmo(true)` → `state='atmo'`.
- **Atmo → land (E)**: `atmoLandStart()` → `state='atmoland'`; when the animation completes → `state='seated'` (still in cockpit).
- **Space → station**: `Station.tryBegin(dt)` in the `space` loop branch → `state='station'`; on `Station.update` returning `'exit'` → `state='space'` (or `btnUndock`).
- **Warp**: in space, with `warpLock` aimed + pulse at speed → `tickWarpAutoJump()` sets `state='warping'` (start `warpAnim`), then `state='warping'` (arrival line 2244), `finishWarp()` → `state='space'`; on arrival the galaxy seed changes and `mapMarks` is swapped with `galaxyArchives[seed+'_marks']`.
- **Quit**: `btnQuit` disconnects net, saves (if possible), `location.reload()` → back to `menu`.

### 7.3 Main-loop timing

`loop()` is driven by `requestAnimationFrame` (**no fixed-timestep accumulator**):

```js
let lastT = performance.now();
function loop(){
  requestAnimationFrame(loop);
  const now = performance.now();
  let dt = Math.min(0.05, (now - lastT) / 1000);   // variable dt, clamped to 50 ms
  lastT = now;
  ...
}
```

- Variable `dt` in seconds, capped at **0.05 s**. Every subsystem uses this same `dt`.
- Early returns: `Net.tick(dt)` runs first (even in menu/loading). Then, if `state==='menu'||state==='loading'` → return. If `paused` (system panel open, single-player) → return (world frozen).
- Per-frame before the state switch: `flushChunkQueue(6)`, `playTime += dt`, `Space.tickRotation(dt)`, `applyAtmoTint(dt)`, re-entry FX cleanup, `tickAtmoScan(dt)`, `Space.tickLasers(dt)`, `tickShipPreview(dt)`.
- Then a big `if/else if` per state: `planet` / `seated` / `atmo` / `atmoland` / `space` / `warping` / `station`, each ending with a `renderer.render(...)` of the appropriate scene.
- **Frame-count assumptions**: the warp animation uses `warpAnim._f++` and constants like `0.016` per frame, `400 frames ≈ 6.7 s`, `540 frames ≈ 9 s`. The galaxy map `galaxyTick()` uses `g3d.t += 1/60`. A Bevy port should either replicate these as fixed 60 Hz assumptions or convert them to time-based.

### 7.4 Complete default key-binding table

Single `keydown` handler (main.js) + `Player.keys[e.code]` for continuous movement + `net.js` for chat/players. `e.repeat` is ignored for E and station-W.

| Key (e.code) | Context | Action |
|---|---|---|
| `KeyW/KeyA/KeyS/KeyD` | planet (Player.keys) | Move forward/left/back/right |
| `Space` | planet | Jump; hold = jetpack |
| `ShiftLeft` | planet (Player.keys) | (sprint modifier where applicable) |
| Mouse L | planet | Mining laser (hold to mine) |
| Mouse R | planet | Place block/machine (ghost preview) |
| `Tab` | any | Toggle inventory/crafting panel |
| `KeyT` | planet | Toggle tech tree |
| `Escape` | any | Close open panel → else open pause menu (Ctrl+Esc = diagnostics) |
| `F8` | any | Toggle runtime diagnostics panel |
| `F5` | any | Quick save |
| `KeyP` | planet/space, creative | Toggle creative item library |
| `KeyC` | planet | Ore/plant scan (pulse + markers) |
| `KeyC` | atmo | Ship POI scan (villages/ruins) |
| `KeyC` | space | Space scan (celestial bodies + POIs) |
| `KeyV` / `KeyM` | space | Open galaxy map |
| `KeyM` | planet / seated / atmo | Open planet map |
| `KeyJ` | space | Pulse engine (hold); also triggers warp when locked & aimed |
| `KeyR` | planet | Rotate placement direction |
| `KeyG` | planet | Throw held item (Shift+G = whole stack) |
| `Digit0` | planet | Select mining laser (fixed slot, `hotIdx=-1`) |
| `Digit1..9` | planet | Select hotbar slot 0–8 |
| `KeyE` | planet | Interact (machines/ship/dialog advance) |
| `KeyE` | station | Station interact (terminals/NPCs) |
| `KeyE` | atmo | Land where possible (also board/exit ship on ground) |
| `KeyW` | seated | Take off (needs fuel or launchpad) |
| `KeyW` | station | Undock from station |
| `KeyW` | space/atmo | Thrust |
| `KeyS` | space/atmo | Brake |
| `ShiftLeft` | space/atmo | Boost |
| `KeyA` | space/atmo | Roll left |
| `KeyD` | space/atmo | Roll right |
| `Enter` | net | Open/send chat |
| `KeyO` | net | Toggle players list |
| Wheel | planet | Cycle hotbar (0–8 then laser) |

Blur (`window.blur`) clears all keys and flight inputs to avoid stuck keys after Alt-Tab.

### 7.5 How `ui.js` hooks into `main.js`

`UI` is a singleton IIFE exposing: `anyPanelOpen, closeAll, toggle, buildHotbar, refreshHotbar, refreshInv, refreshAll, showItemName, openMachinePanel, openTrade, refreshTrade, refreshTech, updateResearch, tickMachinePanel, toggleCreative, openSavePanel, openGalaxyMap, openCharCreate, openWorldCreate, tryCraft, pickupToast, bigMessage, refreshQuests, refreshHUD, setInteractHint, getCursor, setCursor, openMachine, researching`.

- **main.js → UI** calls: `UI.closeAll()`, `UI.buildHotbar()`, `UI.refreshAll()`, `UI.updateResearch(dt)`, `UI.tickMachinePanel(dt)`, `UI.setInteractHint(...)`, `UI.anyPanelOpen()`, `UI.bigMessage(...)`, `UI.toggle('invPanel'|'techPanel'|'pausePanel'|…)`, `UI.openGalaxyMap()`, `UI.openSavePanel(mode)`, `UI.openCharCreate()`, `UI.openWorldCreate()`, `UI.toggleCreative()`, `UI.openTrade()`, `UI.openMachinePanel(m)`, `UI.refreshHUD()`, `UI.refreshQuests()`, `UI.pickupToast()`.
- **main.js monkey-patches** `UI.toggle` to also refresh the ship panel when the inventory opens.
- **UI → Game reads**: `Game.state`, `Game.creative`, `Game.dropMult`, `Game.market`, `Game.flags`, `Game.techDone(id)`, `Game.completeTech(id)`, `Game.lastTech`, `Game.currentQuests()`, `Game.sideQuest()`, `Game.checkQuest()`, `Game.warpLockSeed`, `Game.neighborSeeds()`, `Game.isGalaxyVisited(seed)`, `Game.listChars/listWorlds/listSaves`, `Game.createCharacter`, `Game.saveTo`, `Game.exportSave`, `Game.importSave`, `Game.deleteSave/Char/World`, `Game.loadPair`, `Game.saveBeaconState`, `Game.setWarpLock`.
- **UI → other modules reads**: `Player.inv/stats/hotIdx/credits/pos/CHARGE_DEFS/addItem/countItem/hasItems/payItems/removeItem/sortInventory/spawnDrop/chargeStat/canCharge`, `Factory.machines/power/canMachineAccept/machineInsert/serialize`, `World.getDef/seed/biome/topAt/structures`, and the data tables `ITEMS/RECIPES/RECIPE_BY_ID/TECH/TRADE_GOODS/STATION_BLUEPRINTS/BIOMES/FUEL_VALUE/DEFAULT_PLANETS/HOME_GALAXY_SEED`.
- **Sound** effects invoked by UI directly: `Sound.play('uiOpen'|'uiClose'|'hover'|'uiClick'|'craft'|'uiError'|'research'|'buy'|'coin'|'insert'|'openChest'|'breakBlk')`.

---

## 8. Pause / Esc menu options

Panel `#pausePanel` ("◈ 系统菜单"):

| Button | Action |
|---|---|
| 继续游戏 (`btnResume`) | `UI.closeAll()` + `lockPointer()` |
| 存档 (`btnSave`) | `UI.openSavePanel('save')` |
| 画面设置 (`btnSettings`) | hide pause, show `#settingsPanel`, `refreshSettingsUI()` |
| 联机 (`btnNet`) | open net panel |
| 操作指南 (`btnHelp2`) | hide pause, show help panel |
| 返回主菜单 (`btnQuit`) | disconnect net, auto-save, `location.reload()` |
| 音量 slider (`volSlider`) | 0–100 (default 70) → `Sound.setVolume` |

`Escape` behavior: close any open panel first (`UI.closeAll()` + relock); else toggle `pausePanel`. When a system panel is open, single-player world simulation is frozen (`worldPaused()`).

---

## Appendix — key data tables (for parity)

### Item categories & stack sizes (excerpt)

- `res`: carbon/oxygen/sodium/coal/iron_ore/copper_ore/titanium_ore/gold_ore (stack 250), uranium (100), tritium (500).
- `mat`: iron/copper/titanium/gold/gear/wire (250), circuit/plate (200), data (500), fuel (20), antimatter (10), warpcell (10).
- `blk`: dirt/stone/sand/planks_b/glass_b (250), lamp_b (100), slab_b/metal_b/concrete_b (250).
- `mach`: furnace_b/miner_b/assembler_b/refinery_b/chest_b/wind_b/burner_b/medbay_b (50), belt_b (200), solar_b (100), reactor_b/beacon_b/collector_b (20), launchpad_b/lumberbot_b (10).
- Prices in ₪ are per-item (used for trade base price and tooltips).

### Trade goods (station terminal)

`TRADE_GOODS = carbon, oxygen, sodium, coal, iron_ore, copper_ore, titanium_ore, gold_ore, uranium, tritium, iron, copper, titanium, gold, gear, wire, circuit, plate, data, fuel, glass_b, antimatter, warpcell`.

### Station blueprints

`STATION_BLUEPRINTS = [{tech:'logistics', price:800}, {tech:'power', price:1500}, {tech:'refining', price:3000}, {tech:'nuclear', price:8000}]`.

### Home galaxy & default planets

- `HOME_GALAXY_SEED = 7777`; station position `[700, 200, -500]`.
- `DEFAULT_PLANETS`: id 0 `lush` 始源星 `[0,0,0]` r150; id 1 `desert` 赤沙 `[1800,120,-900]` r130; id 2 `frozen` 霜白 `[-1500,-200,-1700]` r140; id 3 `volcanic` 熔核 `[900,-100,2300]` r120; id 4 `alien` 紫瘴 `[-2400,250,1100]` r145.
- Generated galaxies: 4–7 planets, biome pool of 16, market multipliers 0.75–1.25, planet radius 105–175.
- `BIOMES` has 16 entries (`lush, desert, frozen, volcanic, alien, ocean, crystal, fungal, ashen, amber, ferrous, murk, salt, obsidian, redmoss, hive`), each with `haz` (null/heat/cold/toxic/rad/storm) + `hazRate`, `oreMul`, `sky`, `fog`, `tint`, animal/skywings definitions.

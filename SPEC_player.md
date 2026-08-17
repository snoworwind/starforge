# STARFORGE `player.js` — 1:1 Porting Specification (Rust target)

**Source file:** `js/player.js` (1108 lines). **External data referenced at runtime:** block/`hard` & machine flags in `js/data.js` (`BLOCKS`), item `stack` sizes in `js/data.js` (`ITEMS`), biome `hazRate`/`lava` flags in `js/data.js` (`BIOMES`), `Game.dropMult` / `Game.creative` / `Game.state` in `js/main.js`, `World.getDef/set/raycast/topAt/inBounds/findSpawn/biome` in `js/world.js`, `Creatures.rayHit/damage` in `js/creatures.js`, `Factory.at/place/remove` in `js/factory.js`. All numbers below are the exact values from the code. Values **not** defined in `player.js` are marked **[ext]**.

All units: meters (world units = 1 block), seconds (`dt` in seconds), radians for angles, "HP/shield" are integer segments ("段" = pip/segment).

---

## 1. Player state fields

| Field | Type | Initial value | Notes |
|---|---|---|---|
| `pos` | Vec3 | `(96, 40, 96)` | Feet position. AABB: `[x−W, x+W] × [y, y+H] × [z−W, z+W]` |
| `vel` | Vec3 | `(0, 0, 0)` | Linear velocity (m/s) |
| `yaw` | f32 | `0` | Radians; 0 = facing −Z |
| `pitch` | f32 | `0` | Radians |
| `onGround` | bool | `false` | Re-evaluated every frame |
| `W` | f32 const | `0.3` | Player half-width (AABB width 0.6) |
| `H` | f32 const | `1.8` | Player height |
| `EYE` | f32 const | `1.62` | Camera eye height above feet |

**Survival stats (`stats` object):**

| Stat | max | initial |
|---|---|---|
| `hp` (health) | 8 | 8 |
| `shield` | 6 | 6 |
| `o2` (oxygen) | 100 | 100 |
| `haz` (hazard protection) | 100 | 100 |
| `jet` (jetpack fuel) | 100 | 100 |
| `laser` (mining laser energy) | 100 | 100 |

Other state: `credits = 0` (currency), `dead = false`, `appearance = null` (cosmetic, network-synced). `inv` = 36 slots (`Array(36).fill(null)`), each slot `{item: string, n: int}` or `null`. `hotIdx = -1` means the **fixed mining-laser slot** is selected (not an inventory slot).

---

## 2. Movement physics

### 2.1 Horizontal ground/air movement

```
speed = ShiftLeft held ? 7.2 : 4.5          // sprint = Shift
forward f = (-sin(yaw), 0, -cos(yaw))
right   r = (-f.z, 0, f.x) = (cos(yaw), 0, -sin(yaw))
wish = 0; W: +f; S: -f; D: +r; A: -r
if |wish|>0: wish = normalize(wish) * speed
accel = onGround ? 12.0 : 5.0
vel.x += (wish.x - vel.x) * min(1, accel*dt)   // exponential approach toward wish
vel.z += (wish.z - vel.z) * min(1, accel*dt)
```
There is **no** discrete friction coefficient; it's first-order approach with rate `accel`.

### 2.2 Vertical: jump, gravity, jetpack

- **Gravity** (when not in liquid): `vel.y -= 22*dt`, then `vel.y = max(vel.y, -40)` → **terminal velocity −40 m/s**.
- **Jump** (Space, on ground): `vel.y = 7.4`, `onGround = false`, play `jump`.
- **Jetpack** (Space, in air, `jet > 0`, **not** in liquid):
  - `vel.y = min(vel.y + 33*dt, 8.5)` → thrust **+33 m/s²**, hard cap upward velocity **+8.5 m/s**; net vs gravity = **+11 m/s²** (33−22).
  - Fuel drain: `jet -= 28*dt` (i.e. **28/s**).
  - Start looping `jet` sound.
- Jetpack sound stops when Space released, on ground, or `jet <= 0`.
- **Jetpack regen (grounded):** `jet = min(jetMax, jet + 40*dt)` → **+40/s**.
- **Creative:** `jet` is forced to `jetMax` every frame.

### 2.3 Liquid (water/lava) physics

Liquid membership (1 frame lag, using previous-frame `pos`):
```
liqFeet = def(floor(pos.x), floor(pos.y+0.1), floor(pos.z))
liqEye  = def(floor(pos.x), floor(pos.y+EYE), floor(pos.z))   // EYE=1.62
inLiquid = liqFeet.liquid || liqEye.liquid
```
When `inLiquid` (runs **before** gravity & collision):
```
vel.x *= max(0, 1 - 5*dt)         // horizontal drag (exponential, rate 5/s)
vel.z *= max(0, 1 - 5*dt)
vel.y += (2.6 - vel.y) * min(1, 4*dt)   // buoyancy: asymptotically approaches +2.6 m/s upward (replaces gravity)
if Space: vel.y = min(vel.y + 24*dt, 5.5)  // swim up: +24 m/s², cap +5.5
onGround = false
```
Otherwise (air): gravity branch above applies.

### 2.4 Axis-separated collision + fall damage

```
np = pos
np.x += vel.x*dt;  if collides(np.x, pos.y, pos.z): np.x = pos.x; vel.x = 0
np.z += vel.z*dt;  if collides(np.x, pos.y, np.z): np.z = pos.z; vel.z = 0
np.y = pos.y + vel.y*dt; onGround = false
if collides(np.x, np.y, np.z):
    if vel.y < 0:
        onGround = true
        if vel.y < -12: damage(floor((-vel.y - 12)/4))      // fall damage
        if !wasGround && vel.y < -6: play 'land'
    np.y = pos.y; vel.y = 0
    while collides(np.x, np.y, np.z): np.y += 0.05           // ground snap step 0.05
pos = np
if pos.y < -10: pos.y = 80; damage(2)                        // void teleport + 2 damage
```

`collides(px,py,pz)` checks every integer cell overlapping the AABB `[px−0.3, px+0.3] × [py, py+1.8] × [pz−0.3, pz+0.3]`; a cell blocks if `def.solid`. For `def.lowbox`: if `true` → effective top height `0.2`; if numeric → that height; the player passes **over** it when `py > y + lb` (i.e. it's a step you can stand on).

**Fall damage formula (exact):** with `v = -vel.y` (positive fall speed):
```
damage = floor((v - 12) / 4),  only if v > 12
```
→ 0 dmg for v ≤ 15.99…, 1 dmg at v=16–19.99, 2 dmg at 20–23.99, 3 dmg at 24–27.99, … (1 extra HP per 4 m/s beyond 12).

### 2.5 Lava & hazard damage

- **Lava lake burn:** `if inLiquid && biome.lava: damageTick(dt, 3)` → **3 HP/s** [ext: `biome.lava` is true for the "熔火之地 / volcanic" biome].
- **Void:** `pos.y < -10` → teleport to y=80, `damage(2)`.

---

## 3. Mining laser

### 3.1 Ray & range

- Ray origin = camera world position; direction = camera forward (`camera.getWorldDirection`).
- **Beam raycast range = 22** (for beam truncation / wall impact visual).
- **Mining is only possible if hit distance ≤ 6** (`hit = far && far.dist <= 6 ? far : null`).

### 3.2 Energy drain

On every frame the laser fires (left button held, `hotIdx === -1`, no panel open):
```
drain = (cHit || hit) ? 1.8 : 0.9       // per second
laser = max(0, laser - drain*dt)
if laser <= 0: laserMul = 0.25          // depleted efficiency
else laserMul = 1
```
Creative: no drain, `laserMul` effectively 1 (and `laser` pinned to max each frame).

### 3.3 Block mining / hardness → break time

```
mining.prog += dt / hard * (creative ? 6 : laserMul)
```
`hard` is the block's mining time in seconds [ext table §11]. So:
- **Break time = `hard / laserMul` seconds** (survival, full energy), or `hard / 6` in creative.
- `hard === Infinity` (barrier/bedrock) is **unmineable** (explicitly excluded from mining & the "needs laser" hint).

Progress accumulates only while the same block `(x,y,z)` is continuously targeted; switching target resets `prog` to 0. Sounds: `dig` every 0.22 s; spark particles every 0.12 s (2 × `0xffaa55`). Crack overlay opacity = `prog * 0.55`; scale pulses `1 + sin(prog*40)*0.01`.

### 3.4 Block breaking & drops

On `prog >= 1` → `breakBlock(hit)`:
- Non-machine: `World.set(x,y,z,0)`; also checks the cell above — if it's a `cross` plant, it is also removed and its drops spawned. Then `dropsOf(def)` is spawned.
- **Drop selection** `dropsOf(def)`: for each entry `{item, n, chance}` in `def.drops`, include it if `!chance || random() <= chance`; final count = `n * Game.dropMult` [ext: survival difficulty multiplier — easy ×7, normal ×4, hard ×1; creative ×1].
- Spawn position: block center `(x+0.5, y+0.5, z+0.5)` via `spawnDrop` (see §10.2).
- Particle burst (12 particles, color by `def.key`: grass `0x69b23f`, dirt `0x8a5f3c`, stone `0x8c8c8c`, sand `0xe0d29a`, log `0x6b502f`, leaves `0x3f7d2c`, else `0x999999`).
- `Game.onBlockMined(def)` callback.

**Machine break** (`def.machine`): calls `Factory.remove(x,y,z)`; if it returns a machine object, its contents are refunded as drop entities: `d.in`, `d.fuel`, `d.out`, `d.slots[]`, `d.cargo` (as `carbon`), numeric sub-fields of `d.in`, `d.items[]`. Reactor special-case: `d.fuel` (seconds) → `uranium` count `round(d.fuel/60)` (1 uranium per 60 s). Assembler/refinery mid-craft refund: if `d.prog > 0 && d.recipe`, refund `recipe.in` quantities.

### 3.5 Attacking creatures

Creature hit is checked **before** block hit (`Creatures.rayHit(origin, dir, min(far.dist,22))`).
- Beam endpoint = creature position + `radius*0.5` (default 0.3) up.
- Damage applied in a **0.28 s tick** accumulator (`shootT += dt; if shootT > 0.28: shootT=0; Creatures.damage(...)`):
  - **1 damage** normal survival; **4** creative; **0.5** when laser depleted (`laserMul < 1`).
  - Plays `laserHit`, spawns 3 × `0xff6a55` particles.
- While a creature is targeted, no block mining occurs.

---

## 4. Block placement

### 4.1 Range & target

`placeTarget(camera)` (also drives the ghost preview):
- Selected slot must be an item with a `block` field.
- Raycast forward, **range 6**.
- Target cell = hit cell + face normal: `(hit.x+face[0], hit.y+face[1], hit.z+face[2])`.
- Rejected if out of bounds, or target cell `def.id !== 0` (not air).
- **Self-intersection check:** reject if the target cell overlaps the player AABB cell range:
  ```
  bx0=floor(pos.x-0.3) .. bx1=floor(pos.x+0.3)
  by0=floor(pos.y) .. by1=floor(pos.y+1.8)
  bz0=floor(pos.z-0.3) .. bz1=floor(pos.z+0.3)
  reject if px∈[bx0,bx1] && pz∈[bz0,bz1] && py∈[by0,by1]
  ```

### 4.2 Ghost preview

- Cyan `0x35e0e8` when valid, red `0xff4444` when invalid; opacity "breathes" `0.16 + sin(now*0.006)*0.07`.
- `lowbox` or `machine === 'belt'` renders as a low slab: scale `(1, 0.25, 1)`, y-offset `-0.375`.
- Machines show a direction arrow (yellow `0xffcf4d` cone) rotated per `effectiveDir()`.

### 4.3 Rotation (R key)

`cycleRot()`: `placeDirOverride = (effectiveDir()+1) % 4`; values `0..3` = `东 +X (0)`, `南 +Z (1)`, `西 -X (2)`, `北 -Z (3)`.
`effectiveDir()` = explicit override if set, else `autoDir()` from yaw:
```
fx=-sin(yaw), fz=-cos(yaw)
if |fx| > |fz|: return fx>0 ? 0 : 2
return fz>0 ? 1 : 3
```
The R-set override **persists** across view changes (until changed again).

### 4.4 Placement execution

`tryPlace(camera)`:
- Machine (`bDef.machine`): `Factory.place(px,py,pz, item.block, effectiveDir())`; play `machinePlace`; beacon special-case calls `Game.saveBeaconState()`; `Game.onBlockPlaced(block)`.
- Normal block: `World.set(px,py,pz, bDef.id)`; play `place`; `Game.onBlockPlaced(block)`.
- Non-creative: decrement `sel.n`; remove slot if 0. `swingHeld()` (placement swing animation), refresh UI.
- Invalid placement plays `uiError`.

**Placeable blocks** = any item whose `ITEMS[item].block` maps to a `BLOCKS` def [ext, see §11 table].

---

## 5. Interaction (E key)

`interact()` in `main.js` [ext, but directly consumes `Player.lookTarget`]:
1. Panel open → ignore.
2. `state==='planet'`:
   - Nearest villager within **3.6** → talk.
   - **`Player.lookTarget(camera)` = forward raycast, range 5.** If hit a machine cell (`Factory.at(x,y,z)`) → open machine panel.
   - Ship within **4.5** of player → `interactShip()`.
   - **Recharge:** if `haz < hazMax-5` and has sodium → `recharge('haz')`; if `o2 < o2Max-5` and has oxygen → `recharge('o2')`.
3. `state==='atmo'` → start landing; `state==='seated'` → exit ship.

`lookTarget` range is **5** (shorter than mine/place range 6).

---

## 6. Camera

- **Position:** `camera.position = (pos.x, pos.y + EYE, pos.z)` → eye height **1.62**.
- **Orientation:** `camera.quaternion.setFromEuler(Euler(pitch, yaw, 0, 'YXZ'))` — i.e. `Ry(yaw) · Rx(pitch)`. Explicit YXZ (no accumulated rotation state; avoids gimbal/order artifacts during fast turns).
- **FOV:** `settings.fov`, default **75** (vertical degrees) [ext `main.js`].
- **Pitch clamp** [ext `main.js`, planet branch]: soft limit `PITCH_SOFT = 1.35 rad`, hard asymptote `PITCH_MAX = 1.55 rad` (exponential approach beyond 1.35 — no hard snap). Station uses hard clamp `[-1.55, 1.55]`.
- **Mouse sensitivity:** `s = settings.mouseSens * 0.0024`; `yaw -= mx*s`, `pitch -= my*s`. (movementX/Y spikes > 200 px are discarded.)
- **Head bob / view-model bob** (applies to the held model, not the camera itself):
  - `bobT += dt * rate`, rate = `11` sprint, `8.5` walk, `1.6` idle (only when on ground & moving, i.e. `wish.lengthSq() > 0.5` for "moving").
  - amplitude `bobAmp` = `0.014` moving, `0.004` idle.
  - offsets: `x = baseX + cos(bobT*0.5)*bobAmp*0.6`, `y = baseY + |sin(bobT*0.5)|*bobAmp*1.4`, smoothed toward target at `dt*12`.
- **Reach:** mine **6**, place **6**, interact **5**; laser beam visual raycast **22**.

---

## 7. Hotbar, drop (G), stack merging

### 7.1 Hotbar selection [ext `main.js` + `ui.js`]

- **Slots:** 10 UI slots. Slot "0" = mining laser (`hotIdx = -1`); slots "1"–"9" = inventory indices `0`–`8`.
- **Digit keys:** `0` → `hotIdx = -1`; `1..9` → `hotIdx = n-1`.
- **Wheel:** wrap-around over a virtual 0..9 ring where 9 ≡ laser:
  ```
  cur = hotIdx === -1 ? 9 : hotIdx           // 0..9
  next = (cur + (deltaY>0 ? 1 : -1) + 10) % 10
  hotIdx = next === 9 ? -1 : next
  ```
  (Wheel down increments; cycles …→ 8 → laser → 0 → ….)

### 7.2 Dropping an item (G) — `throwHeld(count)`

- Only if `hotIdx >= 0` and slot non-null.
- Count: `n = min(s.n, count || (Shift held ? s.n : 1))` → 1 item, or whole stack with Shift.
- Throw direction = camera look vector `dir = (-sin(yaw)cos(pitch), sin(pitch), -cos(yaw)cos(pitch))`.
- Spawn at `pos + dir*0.7 (x,z)`, `pos.y - 0.15 + dir.y*0.5`.
- Velocity `(dir.x*6, dir.y*6 + 2.2, dir.z*6)`; `pickDelay = 1.2`.
- Decrement stack; play `uiClick`.

### 7.3 Stack merging

`addItem(item, n)`:
- Max stack = `ITEMS[item].stack` [ext, default 250].
- First fill existing partial stacks of same item (oldest slot first), then empty slots.
- Returns number actually added; plays pickup toast/sound and refreshes UI when `added>0`.

`removeItem(item, n)`: requires `countItem >= n`; removes from **last slot backwards**, nulls empty slots; returns bool.

`sortInventory()`: merges & compacts slots **9–35 only** (hotbar 0–8 untouched): totals per item in first-seen order, split into max-stack chunks, written front-to-back (empty slots sink to the end).

`hasItems/payItems(costs)`: boolean check / atomic multi-item pay for recipes.

---

## 8. Jetpack

- **Capacity:** `jetMax = 100` (continuous float).
- **Thrust:** `+33 m/s²` when Space held in air (not liquid, `jet > 0`); upward velocity capped at **+8.5 m/s**; net climb **+11 m/s²** vs gravity.
- **Drain:** **28/s** while active.
- **Regen:** **+40/s** while `onGround`.
- **Effects:** looping `jet` sound while active (stopped on release/ground/empty/death); no visual particle emitter in `player.js`.
- Creative mode: fuel pinned to max (effectively infinite).

---

## 9. Damage system

### 9.1 Application

`damage(n)` (integer):
- Ignored if dead, `n <= 0`, or creative.
- Plays `hurt`, flashes `#damageFlash` for 150 ms.
- Order: deplete **shield first** (1 per point), then **hp**.
- `hp <= 0` → `die()`.

### 9.2 Continuous damage accumulator

`damageTick(dt, rate)`: `dmgAcc += dt*rate;` each whole 1.0 → `damage(1)` (remainder preserved). Uses a single shared `dmgAcc` across sources.

| Source | Rate (HP/s) |
|---|---|
| Oxygen exhausted (`o2 <= 0`) | 0.5 |
| Hazard exhausted (`haz <= 0`) | 0.4 |
| Lava lake (`inLiquid && biome.lava`) | 3 |
| Void fall (`y < -10`) | instant 2 |

### 9.3 Shield / health regeneration

- **Shield regen:** `+0.15/s` while `o2 > 20 && haz > 10` (capped at `shieldMax`). (This is the only regen gate; there is no separate timed "regen delay".)
- **HP** does **not** regenerate passively in `player.js` (only via charging/medbay [ext]).
- **Hazard regen:** `+2/s` when current biome has no hazard (`!biome.haz`), capped.

### 9.4 Death & respawn

`die()`:
- `dead = true`; stop jet/laser loops; play `alarm`; message "信号丢失 / 外骨骼将在重生点重建…物资保留" (supplies preserved).
- Fade in (`#fader.show`); after **1800 ms**:
  - `pos = World.findSpawn()`; `vel = 0`.
  - `hp, shield, o2, haz, jet, laser` all reset to max.
  - `dead = false`; fade out.
- **No inventory/credit penalty on death** (items are kept).

### 9.5 Recharging via items (`CHARGE_DEFS`)

| System | Item | cost | gain | cap |
|---|---|---|---|---|
| `laser` | carbon | 3 | +30 | laserMax |
| `shield` | sodium | 2 | +2 | shieldMax |
| `hp` | oxygen | 4 | +2 | hpMax |
| `o2` | oxygen | 1 | +30 | o2Max |
| `haz` | sodium | 1 | +25 | hazMax |

`canCharge` requires `stat < max - 0.01` and enough items; `chargeStat` removes cost and adds gain. `recharge('haz')` / `recharge('o2')` (E-key path) only trigger when `stat < max - 5`.

---

## 10. Inventory & dropped-item entities

### 10.1 Inventory

- **Capacity:** 36 slots; **hotbar = slots 0–8**; **storage = slots 9–35**.
- **Stack sizes** [ext `ITEMS`]: default **250**; exceptions: `circuit`/`plate` 200, `lamp_b` 100, `uranium` 100, `tritium` 500, `data` 500, `fuel` 20, `antimatter` 10, `warpcell` 10, `furnace_b/miner_b/assembler_b/refinery_b/chest_b/wind_b/burner_b/medbay_b` 50, `solar_b` 100, `belt_b` 200, `reactor_b` 20, `launchpad_b` 10, `lumberbot_b` 10, `collector_b` 20, `beacon_b` 20. (Full list §11.)
- Add: merge → empty slot; Remove: from tail backward; Sort: storage only.

### 10.2 Dropped-item entities (`spawnDrop`, `updateDrops`)

- **Mesh:** shared `PlaneGeometry(0.46, 0.46)`, per-item `CanvasTexture` icon (NearestFilter, `alphaTest: 0.4`, DoubleSide).
- **Merge:** a new drop merges into an existing same-item drop within `distanceToSquared < 1.2` (≈ 1.095 m) — count added, `age` reset.
- **Cap:** 90 live drops; exceeding shifts the oldest: `addItem(old.item, old.n, silent)`; remainder (if inventory full) is re-pushed to the tail with the leftover count (no loss).
- **Physics:** gravity **16 m/s²** (`vel.y -= 16*dt`); only integrates while `vel.lengthSq() > 0.0001`.
  - Land on solid at `p.y - 0.28` → snap `y = floor(p.y-0.28)+1+0.3`, vel=0.
  - `y < -8` → teleport to `World.topAt(floor(x),floor(z)) + 0.4`, vel=0.
  - Resting bob: `y = baseY + sin(age*2.2)*0.06 + 0.06`; re-falls if the block under `baseY-0.4` is removed (vel.y set to −0.5).
  - Spin: `rotation.y += dt*1.6`.
- **Pickup / magnet:**
  - Active only when `age > pickDelay` (default **0.4**, thrown items **1.2**).
  - Target distance measured to `pos + (0, -1.0, 0)` (player chest, 1.0 below feet).
  - If `dist < 6.5` and inventory can accept: if `dist > 1.05` → magnet fly toward chest at `spd = min(26, 8 + (6.5-dist)*4)` m/s (no overshoot); else (≤1.05) → `addItem`.
  - Full inventory → `noSpace` flag, retry after **1.5 s**.
- **Despawn:** `age > 240` (4 minutes).
- **Initial velocity** (auto-spawned drops): `((rand-0.5)*2.2, 2.6, (rand-0.5)*2.2)` horizontal ±1.1, up +2.6.

---

## 11. Complete numeric constants table

### 11.1 Defined in `player.js`

| Constant | Value | Meaning |
|---|---|---|
| spawn `pos` | (96, 40, 96) | Initial position |
| `W` | 0.3 | AABB half-width |
| `H` | 1.8 | AABB height |
| `EYE` | 1.62 | eye height |
| `hpMax` / init hp | 8 | |
| `shieldMax` / init | 6 | |
| `o2Max` / init | 100 | |
| `hazMax` / init | 100 | |
| `jetMax` / init | 100 | |
| `laserMax` / init | 100 | |
| walk speed | 4.5 | |
| sprint speed (Shift) | 7.2 | |
| ground accel | 12 /s | |
| air accel | 5 /s | |
| jump velocity | 7.4 | |
| gravity | 22 m/s² | |
| terminal velocity | −40 m/s | |
| jetpack thrust | +33 m/s² | |
| jetpack up cap | +8.5 m/s | |
| jetpack drain | 28 /s | |
| jetpack regen (ground) | 40 /s | |
| water drag rate | 5 /s | |
| buoyancy target | +2.6 m/s | |
| swim-up accel | +24 m/s² | |
| swim-up cap | +5.5 m/s | |
| fall-damage threshold | 12 m/s | `floor((v-12)/4)` |
| land-sound threshold | 6 m/s | |
| ground snap step | 0.05 | |
| void teleport | y<−10 → y=80, +2 dmg | |
| o2 drain | 0.35 /s | |
| haz regen (safe biome) | +2 /s | |
| o2-empty damage | 0.5 HP/s | |
| haz-empty damage | 0.4 HP/s | |
| shield regen | +0.15 /s (o2>20 & haz>10) | |
| lava damage | 3 HP/s | |
| laser beam raycast | 22 | |
| laser mine range | 6 | |
| laser energy drain (hit) | 1.8 /s | |
| laser energy drain (air/wall) | 0.9 /s | |
| laser depleted multiplier | 0.25 | |
| creative mining speed | ×6 | |
| creature damage tick | 0.28 s | |
| creature damage | 1 (creative 4, depleted 0.5) | |
| place/interact reach | 6 / 5 | |
| drop gravity | 16 m/s² | |
| drop initial vel | horiz ±1.1, up +2.6 | |
| drop merge radius² | 1.2 | |
| drop magnet radius | 6.5 | |
| drop pickup radius | 1.05 | |
| drop magnet speed | 8 + (6.5−d)*4, cap 26 | |
| drop pickDelay | 0.4 (throw 1.2) | |
| drop cap | 90 | |
| drop despawn | 240 s | |
| drop spin | 1.6 rad/s | |
| drop no-space retry | 1.5 s | |
| drop mesh size | 0.46 | |
| inventory slots | 36 | |
| hotbar slots | 9 (idx 0–8) | |
| fall-damage `dmgAcc` step | 1.0 | |
| death respawn delay | 1800 ms | |
| bob rates | 11 / 8.5 / 1.6 | |
| bob amplitude | 0.014 / 0.004 | |
| FOV default | 75° [ext] | |
| pitch soft / max | 1.35 / 1.55 rad [ext] | |
| mouse sens factor | 0.0024 [ext] | |

### 11.2 Block hardness `hard` (seconds) — `BLOCKS` [ext `data.js`]

| block | hard | notes |
|---|---|---|
| sodium_plant, oxygen_plant, fern, glow_shroom | 0.05 | cross plants |
| leaves | 0.3 | |
| glass | 0.4 | |
| lamp, belt, mush_cap | 0.5 | belt = machine lowbox |
| sand | 0.6 | |
| dirt, snow, salt, redmoss, alien, murk | 0.7 | |
| grass, ash, crystal_plant… | 0.75 (grass/alien/murk/redmoss 0.75; ash 0.8) | |
| planks, chest, collector | 0.9 | |
| wind, rust, slab, lumberbot | 1.0 | |
| log, hive | 1.1 | |
| ice, furnace, miner, burner | 1.2 | |
| amber | 1.4 | |
| assembler, medbay | 1.4 | |
| stone, concrete, refinery | 1.6 | |
| crystal | 1.8 | |
| basalt, metal, launchpad | 2.0 | |
| coal_ore | 2.2 | |
| reactor | 2.4 | |
| iron_ore, copper_ore, obsidian | 2.6 | |
| gold_ore | 3.0 | |
| titanium_ore | 3.6 | |
| uranium_ore | 4.2 | |
| barrier | ∞ | unmineable |

(Full list in `js/data.js` lines 12–68; `hard` = seconds to mine at full laser power, survival.)

### 11.3 Item stack sizes `stack` — `ITEMS` [ext `data.js`]

| stack | items |
|---|---|
| 250 (default) | carbon, oxygen, sodium, dirt, stone, sand, coal, iron_ore, copper_ore, titanium_ore, gold_ore, iron, copper, titanium, gold, gear, wire, planks_b, glass_b, slab_b, metal_b, concrete_b |
| 200 | circuit, plate, belt_b |
| 500 | tritium, data |
| 100 | uranium, lamp_b, solar_b |
| 50 | furnace_b, miner_b, assembler_b, refinery_b, chest_b, wind_b, burner_b, medbay_b |
| 20 | fuel, reactor_b, collector_b, beacon_b |
| 10 | antimatter, warpcell, launchpad_b, lumberbot_b |

### 11.4 Biome hazard drain rates `hazRate` [ext `data.js`]

| biome | hazRate |
|---|---|
| murk (荧光沼泽) | 1.1 |
| redmoss (红藓高原) | 1.1 |
| amber (金珀沙海) | 1.2 |
| fungal (巨菌之森) | 1.3 |
| frozen (冰封世界) | 1.4 |
| ferrous (磁暴铁原) | 1.5 |
| hive (蜂窝穹丘) | 1.5 |
| desert (灼热荒漠) | 1.6 |
| crystal (晶簇冻土) | 1.7 |
| alien (异星菌境) | 1.8 |
| obsidian (黑曜熔壁) | 1.9 |
| ashen (灰烬荒原) | 2.0 (radiation) |
| volcanic (熔火之地) | 2.2 (also `lava: true`) |

### 11.5 Difficulty drop multiplier `dropMult` [ext `main.js`]

| mode | multiplier |
|---|---|
| Easy | ×7 |
| Normal | ×4 |
| Hard | ×1 |
| Creative | ×1 |

---

## Notes for the Rust port (gotchas)

1. **Frame order matters:** drops update → movement → liquid (before gravity/collision) → axis-separated collision → void check → lava/hazard/survival → camera → mining/ghost/particles. Liquid detection uses **previous-frame position** (1-frame lag) for feet/eye.
2. **Yaw convention:** forward = `(-sin yaw, 0, -cos yaw)`; yaw 0 faces −Z. Rotation quaternion is **YXZ** `Euler(pitch, yaw, 0)`.
3. **`damageTick` accumulator is shared** and keeps its fractional remainder (do not round to zero).
4. **Fall damage** uses `-12` in the formula, but the effective 1-damage start is exactly **−16 m/s** due to the floor.
5. **Jetpack is disabled in liquid**; swimming replaces it.
6. **Drops** use gravity 16 (not player gravity 22); the "still" branch re-checks support only when speed ≤ ~0.01 m/s — set vel.y to exactly −0.5 when the floor is removed (a value whose squared length is above the 0.0001 gate).
7. **`hard === Infinity`** must be treated as unmineable everywhere (mining, hint, beam target).
8. **Beam** (22) and **mine/place** (6) are different raycasts; **interact** uses 5.
9. Hotbar digit "0" = laser (`hotIdx = -1`), and wheel treats the laser as virtual index 9.

---

I also verified the exact block hardness, item stack, biome hazard, and difficulty-multiplier values live outside `player.js` (in `js/data.js` and `js/main.js`) and included them in §11 so the Rust port can be implemented without a second source-grep pass.

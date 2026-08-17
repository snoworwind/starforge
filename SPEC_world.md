# STARFORGE `world.js` — Technical Specification for 1:1 Rust Port

> Repository note (2026-08-17): all legacy paths named below now live under `legacy-web/`.

Scope: `js/world.js` (the voxel terrain/world module). It depends on three external definitions reproduced here because they are required for a faithful port: `mulberry32` (`textures.js:7`), the block table `BLOCKS` (`data.js:10`), the biome table `BIOMES` (`data.js:201`), and the texture atlas `Tex` (`textures.js`).

---

## 1. Constants and every hardcoded value

### 1.1 World constants (module-level)

| Name | Value | Meaning |
|---|---|---|
| `CHUNK` | `16` | Chunk side length in blocks (X and Z) |
| `WORLD_H` | `96` | World height in blocks (Y from 0..95) |
| `SEA` | `32` | Sea surface level (water top is at y=SEA inclusive) |
| `SEA_Y` | `SEA - 4 = 28` | "Neutral height": sphere-attach reference, used as curvature grow anchor, map-relief anchor |
| `CHUNK_CELLS` | `16*16*96 = 24576` | Block count per chunk; chunk data is `Uint8Array(24576)` |
| `GEN_R` | `17` | Chunk-generation radius (Chebyshev), default |
| `MESH_R` | `16` | Mesh-build radius (Chebyshev), default — chunks actually rendered |
| `UNLOAD_R` | `19` | Mesh-unload radius (Chebyshev), default |
| `shadowsOn` | `false` | Whether chunk meshes cast/receive shadows (high quality) |

**Curvature radius.** The code comment states the voxel curvature radius is constant `1/0.004 = 250` blocks. There is no literal `0.004` in code; the `0.004` is the curvature `1/R`. The actual shader displacement coefficient is `0.002` (see §7). With `uCurveAmt=1`, a paraboloid `y -= r²·0.002` has curvature radius `R = 1/(2·0.002) = 250` blocks, consistent with the comment.

### 1.2 View-distance linkage (`setViewDist(n)`)

```
MESH_R   = n
GEN_R    = n + 1
UNLOAD_R = n + 3
r1 = n * 16 - 8                // inner radius of far-hole (blocks)
r0 = max(56, r1 - 90)          // outer radius of far-hole (blocks)
farHoleU.r0 = r0 * r0          // stored as radius²
farHoleU.r1 = r1 * r1
```
Defaults at startup: `farHoleU = { r0: 158² = 24964, r1: 248² = 61504 }` (consistent with `MESH_R=16`). `main.js:53` calls `setViewDist(settings.chunkDist >= 33 ? 64 : settings.chunkDist)`.

### 1.3 Far simulated terrain constants

| Name | Value | Meaning |
|---|---|---|
| `FAR_STEP` | `12` | blocks per far-mesh cell (adjustable via `setFarDist`) |
| `FAR_N` | `129` | far mesh is 129×129 vertices |
| default coverage | `(129-1)*12 = 1536` blocks ≈ 1536-block view distance |
| far-row refresh budget | `10` rows per `tickFar` call |
| far snap | player position snapped to multiple of `64` blocks |

`setFarDist(dist)`: `FAR_STEP = max(4, round(dist*2/(FAR_N-1))) = max(4, round(dist/64))`.

### 1.4 Material constants

- `solidMat`: `MeshLambertMaterial({ map: Tex.texture, vertexColors: true, transparent: true, alphaTest: 0.4, side: DoubleSide })`
- `waterMat`: `MeshLambertMaterial({ map: Tex.texture, vertexColors: true, transparent: true, opacity: 0.72, side: DoubleSide })`
- Water wave amplitude `0.035` (two terms), water top-face inset `0.12`.
- Per-face bake shades: `+Y=1.0`, `-Y=0.5`, `±X=0.8`, `±Z=0.65`.
- Water vertex shade: `0.72 + faceShade*0.28`.
- Glow-block self-light multiplier: `2.2`; glow plant (cross) brightness `1.7`.
- Lamp scan period `0.5 s`, lamp search radius² `3600` (=60 blocks), lamp pool size `6`, lamp intensity `0.95`.

### 1.5 GLOW tables

```
GLOW_EMIT (emissive RGB, additive after fog):
  lamp:        [0.62, 0.48, 0.24]
  crystal:     [0.20, 0.60, 0.54]
  glow_shroom: [0.16, 0.55, 0.38]
  amber:       [0.30, 0.22, 0.10]

GLOW_LIGHT (point-light color, hex — only these enter the lamp pool):
  lamp:        0xffd9a0
  crystal:     0x7fe8e0
  glow_shroom: 0x4ee8b8
```
Note: `amber` is in `GLOW_EMIT` (self-emissive terrain) but **not** in `GLOW_LIGHT` (not a point light).

---

## 2. Noise implementation (exact math)

### 2.1 `mulberry32(seed)` — from `textures.js:7`

```javascript
function mulberry32(seed) {
  let a = seed >>> 0;                    // unsigned 32-bit
  return function() {
    a |= 0; a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;   // [0,1)
  };
}
```
All arithmetic is 32-bit signed/unsigned via `|0`, `>>>0`, and `Math.imul` (32-bit multiply). Rust: use `u32`/`i32` wrapping arithmetic. `a + 0x6D2B79F5` uses wrapping 32-bit add. `1 | a` is bitwise OR of the *number* 1 with `a`.

### 2.2 `makeNoise(seed)` — 2D gradient (Perlin) noise

```javascript
function makeNoise(seed) {
  const rnd = mulberry32(seed);
  const perm = new Uint8Array(512);      // uint8 permutation table
  const p = [];                           // p[i] = i, i in 0..255
  for (let i = 0; i < 256; i++) p[i] = i;
  // Fisher–Yates shuffle using rnd
  for (let i = 255; i > 0; i--) {
    const j = (rnd() * (i + 1)) | 0;
    [p[i], p[j]] = [p[j], p[i]];
  }
  for (let i = 0; i < 512; i++) perm[i] = p[i & 255];

  function fade(t) { return t * t * t * (t * (t * 6 - 15) + 10); }   // quintic smoothstep
  function grad2(h, x, y) {
    switch (h & 3) {
      case 0: return  x + y;
      case 1: return -x + y;
      case 2: return  x - y;
      default: return -x - y;
    }
  }
  function n2(x, y) {
    const X = Math.floor(x) & 255, Y = Math.floor(y) & 255;
    x -= Math.floor(x); y -= Math.floor(y);          // fractional parts
    const u = fade(x), v = fade(y);
    const a = perm[X] + Y, b = perm[X + 1] + Y;      // NOTE: perm values are uint8; a,b are indices 0..511
    return lerp(
      lerp(grad2(perm[a],     x,     y),     grad2(perm[b],     x - 1, y),     u),
      lerp(grad2(perm[a + 1], x,     y - 1), grad2(perm[b + 1], x - 1, y - 1), u),
      v);
  }
  function fbm2(x, y, oct = 4, lac = 2, gain = 0.5) {
    let amp = 1, f = 1, sum = 0, norm = 0;
    for (let i = 0; i < oct; i++) {
      sum += n2(x * f, y * f) * amp;
      norm += amp;
      amp *= gain; f *= lac;
    }
    return sum / norm;               // normalized to approx [-1, 1]
  }
  return { n2, fbm2 };
}
```

**Port-ready pseudocode notes:**
- `lerp(a,b,t) = a + (b-a)*t` (THREE.MathUtils.lerp).
- `perm` is `uint8` (values 0..255), but `perm[a]` and `perm[a+1]` are used to index `perm` again (classic Perlin double-index). Since values are 0..255, the second index is in-range.
- `fbm2` returns **normalized** sum (divides by total amplitude), so output is roughly in `[-1, 1]`. Everywhere it is used, callers apply `*0.5+0.5` to map to `[0,1]` except where noted (e.g. `ridge`/`spire` use raw signed values).
- Default `oct=4, lac=2, gain=0.5` unless a call overrides.

### 2.3 3D value noise (caves)

```javascript
function lattice3(x, y, z, salt) {   // deterministic hash -> [0,1)
  let h = (seed ^ salt) >>> 0;
  h = Math.imul(h ^ x, 374761393);
  h = Math.imul(h ^ y, 217645177);
  h = Math.imul(h ^ z, 668265263);
  h = Math.imul(h ^ (h >>> 15), 2246822519);
  return ((h ^ (h >>> 13)) >>> 0) / 4294967296;
}
function vnoise3(x, y, z, salt) {
  const ix = Math.floor(x), iy = Math.floor(y), iz = Math.floor(z);
  let fx = x - ix, fy = y - iy, fz = z - iz;
  fx = fx*fx*(3 - 2*fx); fy = fy*fy*(3 - 2*fy); fz = fz*fz*(3 - 2*fz);  // smoothstep
  // 8 corners
  const c000 = lattice3(ix,iy,iz,salt), c100 = lattice3(ix+1,iy,iz,salt);
  const c010 = lattice3(ix,iy+1,iz,salt), c110 = lattice3(ix+1,iy+1,iz,salt);
  const c001 = lattice3(ix,iy,iz+1,salt), c101 = lattice3(ix+1,iy,iz+1,salt);
  const c011 = lattice3(ix,iy+1,iz+1,salt), c111 = lattice3(ix+1,iy+1,iz+1,salt);
  // trilinear interpolation with smoothstep weights
  const x00 = c000 + (c100-c000)*fx, x10 = c010 + (c110-c010)*fx;
  const x01 = c001 + (c101-c001)*fx, x11 = c011 + (c111-c011)*fx;
  const y0 = x00 + (x10-x00)*fy, y1 = x01 + (x11-x01)*fy;
  return y0 + (y1-y0)*fz;
}
```
Note `x`,`y`,`z` here are integers (world coords), `salt` is an integer. `vnoise3` is value noise (hash at lattice points, trilinear interpolation), unlike `n2` which is gradient noise.

### 2.4 `hash2(x, z, salt)` — deterministic per-column/chunk RNG

```javascript
function hash2(x, z, salt = 0) {
  let h = (seed ^ salt) >>> 0;
  h = Math.imul(h ^ (x | 0), 374761393);
  h = Math.imul(h ^ (z | 0), 668265263);
  h = (h ^ (h >>> 13)) >>> 0;
  return mulberry32(h);      // returns an rnd() closure
}
```

---

## 3. Chunk layout, coordinates, storage

### 3.1 Coordinate system

- World space: integer block coords `(x, y, z)`. `x` and `z` are horizontal, `y` is vertical `0..WORLD_H-1`.
- Chunk coords: `cx = cf(x) = floor(x / CHUNK)`, `cz = cf(z) = floor(z / CHUNK)`.
- Local coords in a chunk: `lx = x - cx*CHUNK` (0..15), `lz = z - cz*CHUNK` (0..15), `y` unchanged.
- `cf(v) = Math.floor(v / CHUNK)`. For negative `x`, JS `floor` correctly maps (e.g. `x=-1 → cx=-1, lx=15`).

### 3.2 Chunk key formats

```javascript
ckey(cx, cz)   = cx * 65536 + cz        // integer key, valid for |cx|,|cz| < 99 (no collision); used in the live Map
strkey(cx, cz) = cx + "," + cz          // string key, used ONLY at save/network boundaries; format is immutable
```

### 3.3 Chunk data array layout

- `c.data` is a **`Uint8Array(CHUNK_CELLS)`** = 24576 bytes (one byte per block; block IDs are 0..59, so 8 bits suffice). The user's question "Uint16 indices?" — **no, the live voxel data is Uint8**; Uint16 is used only for the saved RLE pair encoding (§10).
- Index function: `lidx(lx, y, lz) = (y * CHUNK + lz) * CHUNK + lx = y*256 + lz*16 + lx`.
- Layout order: **X fastest, then Z, then Y major**. Equivalently each horizontal Y-slice is a 16×16 array stored `z`-major / `x`-minor; the 256 blocks of one column `(lx,lz)` are at stride 256 in Y.
- World→index: `lidx(x - cf(x)*16, y, z - cf(z)*16)`.

### 3.4 Chunk object shape

```javascript
c = { cx, cz, data: Uint8Array(24576),
      mesh: null, waterMesh: null,     // THREE.Mesh or null
      dirty: true,                      // needs (re)mesh
      modified: false,                  // ever edited by player (or loaded from save-mods)
      // additional transient flags:
      // needSave: bool (full snapshot pending), fromSave: bool (loaded from v4 snapshot)
      // lamps: null | [[x,y,z,key], ...] (glow light positions, computed at mesh time)
    };
```

### 3.5 Compression for saves (RLE)

```javascript
function rleEncode(data) {          // returns JS array [run, id, run, id, ...]
  const out = [];
  let cur = data[0], run = 1;
  for (let i = 1; i < data.length; i++) {
    if (data[i] === cur && run < 65535) run++;
    else { out.push(run, cur); cur = data[i]; run = 1; }
  }
  out.push(run, cur);
  return out;
}
function rleEncode16(data) { return Uint16Array.from(rleEncode(data)); }   // run ≤ 65535
```
- Pairs are `[runLength, blockId]`, run ≤ 65535, **flattened** (not arrays-of-pairs). Decoding walks the flat array two-at-a-time.
- `rleDecode(data, pairs)`: sums `pairs[p]` for even `p`; if total ≠ `data.length` returns `false` (corruption / world-height change); otherwise `data.fill(pairs[p+1], i, i+pairs[p])` per run.

---

## 4. Terrain generation for one chunk (exact order)

`genChunk(cx, cz)` does, in this exact order:

### Step 0 — cache + snapshot/mods short-circuit
1. If chunk exists, return it.
2. `genCount++` (diagnostic). Allocate `c` (data zeroed, `dirty=true`).
3. **v4 full snapshot**: if `savedChunks` has `strkey(cx,cz)`, `rleDecode(c.data, rec.data)`; on success set `fromSave=true`, `modified=!!rec.mod`, `needSave=false`, `markNeighborsDirty`, return. On decode failure, warn and fall through to procedural.
4. **Modified-chunk RLE** (server authority): if `savedMods[strkey]` exists, fill `c.data` from runs; `modified=true`, `needSave=false`, `markNeighborsDirty`, return.
5. Otherwise **procedural generation**:

### Step 1 — setup
```
grassId = BLOCKS[biome.grass].id
dirtId  = BLOCKS[biome.dirt].id
deepId  = BLOCKS[biome.deep].id
stoneId = BLOCKS.stone.id            // always id 3
x0 = cx*16, z0 = cz*16
SEAB = SEA + (biome.seaLift || 0)    // effective sea level
noBeach = ['sand','basalt','ash','salt','obsidian','rust','hive','amber'].includes(biome.grass)
floraList = biome.flora || ['sodium_plant','oxygen_plant','fern']
```

### Step 2 — base terrain (per column, per Y)
For `lz in 0..15`, `lx in 0..15`:
```
wx = x0+lx, wz = z0+lz
h  = heightAt(wx, wz)                 // §4.1
sd = subDefAt(wx, wz)                 // §4.2
surfId = (sd && sd.g) ? BLOCKS[sd.g].id : grassId
canCave = h > SEAB + 1
cr = hash2(wx, wz, 0x51CA)            // column RNG (cave/decor shared)
for y in 0..h:
    id =
      y == 0      → BLOCKS.barrier.id          (24, unbreakable bedrock)
      y == h      → (h < SEAB+1 && !noBeach) ? BLOCKS.sand.id : surfId
      y > h-3     → dirtId                     (top 3 layers below surface)
      y < 10      → deepId
      else        → stoneId
    if canCave && y >= 3 && y <= h-3 && isCave(wx,y,wz):   // §4.4
        id = (biome.key=='crystal' && cr() < 0.12) ? BLOCKS.crystal.id : 0   // geodes get crystal lining
    data[lidx(lx,y,lz)] = id
```
So the vertical stratification (top→bottom) is: surface (surfId or sand beach) → dirt (3 thick) → stone → deep (below y=10) → barrier (y=0). Caves carve air (or crystal lining) between y=3 and h-3.

### Step 3 — water / lava fill
```
if h < SEAB && (!biome.dry || biome.lava):
    for y in h+1 .. SEAB:  data[lidx(lx,y,lz)] = BLOCKS.water.id   // id 16
```
**Important:** lava is the *same block* (`water`, id 16). Volcanic/obsidian lakes are water blocks tinted orange via `biome.waterTint` (`volcanic: 0xff6a1a`). `biome.dry` suppresses water unless `biome.lava` is set (`volcanic.lava = true`).

### Step 4 — column decorations (deterministic, uses `cr()`)
Only when `h > SEAB`:
```
rv = cr()
if rv < 0.0015:                                     // surface ore outcrop
    oid = cr() < 0.5 ? iron_ore : copper_ore
    data[lidx(lx,h,lz)] = oid
    if cr() < 0.6 && h > 1: data[lidx(lx,h-1,lz)] = oid
else if biome.crystals && rv < 0.0015 + biome.crystals:   // tritium crystal spire
    ch = 1 + (cr()*3 | 0)                           // 1..3
    for y in 1..ch (while h+y < 96): data[lidx(lx,h+y,lz)] = crystal
else if rv < 0.0015 + biome.flowers*(sd.f ?? 1) && !treeAt(wx,wz)
        && data[lidx(lx,h,lz)] == surfId:
    pick = floraList[(cr()*floraList.length)|0]
    data[lidx(lx,h+1,lz)] = BLOCKS[pick].id
decorColumn(...)                                     // §4.6
```
Else if `biome.key == 'ocean'` (underwater coral, when `h <= SEAB`):
```
if cr() < 0.045 && h+1 < 96:
    pick = cr()<0.5 ? 'glow_shroom' : (cr()<0.5 ? 'sodium_plant' : 'fern')
    data[lidx(lx,h+1,lz)] = BLOCKS[pick].id
```

### Step 5 — floating islands (`alien` biome only)
For each column:
```
fl = floatIslandAt(noise, wx, wz)     // §4.3; may be null
if fl:
    gh = heightAt(wx, wz)
    if gh + 6 > fl.base: continue     // skip if ground is too high (avoid embedding)
    for y in fl.base .. fl.base+fl.thick (while y<96):
        id = (y == fl.base) ? alien : (y < fl.base+3 ? dirt : stone)
    if fl.base+fl.thick < 96 && hash2(wx,wz,0xF10A)() < 0.4:
        data[lidx(lx, fl.base+fl.thick, lz)] = alien    // island-top mycelium cap
```

### Step 6 — ore veins (§4.5)
Chunk RNG `rng = hash2(cx, cz, 0x0DE5)`. For each ore in the fixed table, place `n` veins by random walk (replaces only `stone` or `deep`).

### Step 7 — trees / giant mushrooms (§4.7)
Loop `lz in -2..CHUNK+2`, `lx in -2..CHUNK+2` (extended range so cross-chunk canopies are written into adjacent chunks that already exist, or clipped if not). Uses `treeAt`.

### Step 8 — structures (§4.8)
`stampStructures(c, x0, z0)` — villages/ruins stamped **after** vegetation to guarantee cleared interiors.

### Step 9 — finalize
```
c.needSave = true          // programmatic chunk: write full snapshot (Minecraft-style)
markNeighborsDirty(cx, cz)
return c
```
`markNeighborsDirty(cx,cz)`: for the 4 neighbors, if neighbor exists **and** has a mesh, `markDirty(n)` (so border face-culling is re-evaluated).

---

### 4.1 `heightAt(wx, wz)` — exact per-biome formulas

`T = biome.terrain.type` (default `'continental'`); `ch = charAxes` (§4.0); `rugged = ch.rugged`.

`CONTINENT_AMP = { continental:12, dunes:8, mesa:8, volcanic:4, glacial:8, flats:4, shatter:6, hive:6, alien:6, archipelago:0, swamp:3 }`.

| Type | Formula (all in floating point, `SEA = 32`) |
|---|---|
| `dunes` | `q = warpXZ(noise,wx,wz,210)`; `base = fbm2(q0*0.0052, q1*0.0052, 5)*0.5+0.5`; `ripple = sin(wx*0.016 + fbm2(wx*0.004,wz*0.004,3)*2.6)`; `h = SEA-2 + base*24*rugged + ripple²*7*rugged`; then `+ craterField(wx,wz,{cell:90, chance:0.12, r0:0.16, r1:0.38, rim:10, floor:10})` |
| `mesa` | `q = warpXZ(noise,wx,wz,150)`; `steps = 3 + (temp*2\|0)`; `v = fbm2(q0*0.0042,q1*0.0042,5)*0.5+0.5`; `v = round(v*steps)/steps`; `h = SEA-8 + v*44*rugged`; `+ fbm2(wx*0.05,wz*0.05,3)*2` |
| `volcanic` | `q = warpXZ(noise,wx,wz,130)`; `b = fbm2(q0*0.0065,q1*0.0065,5)`; `ridge = max(0, 1 - abs(fbm2(q0*0.0105+40, q1*0.0105,4))*1.7 - 0.18)`; `basin = fbm2(wx*0.006+55, wz*0.006-21, 2)*0.5+0.5`; `h = SEA-10 + ridge*52*rugged + b*10 + (basin-0.5)*20`; `+ spireField(noise,wx,wz,0.008,0.58,160,24)*rugged`; `+ craterField(wx,wz,{cell:110, chance:0.3, r0:0.14, r1:0.34, rim:13, floor:16})` |
| `archipelago` | `q = warpXZ(noise,wx,wz,240)`; `v = fbm2(q0*0.0065,q1*0.0065,3)*0.5+0.5`; `m = max(0,(v-0.47)/0.17)`; `h = SEA-12 + pow(m,1.5)*60*rugged`; `+ fbm2(wx*0.03,wz*0.03,3)*2.5` |
| `glacial` | `b = fbm2(wx*0.0042,wz*0.0042,4)*0.5+0.5`; `ridge = 1 - abs(fbm2(wx*0.008+9, wz*0.008,4))`; `h = SEA-2 + b*14*rugged + pow(ridge,3)*26*rugged` |
| `flats` | `b = fbm2(wx*0.004,wz*0.004,4)*0.5+0.5`; `h = SEA-1 + (b-0.5)*10*rugged`; `+ craterField(wx,wz,{cell:120, chance:0.22, r0:0.12, r1:0.3, rim:7, floor:9})` |
| `swamp` | `q = warpXZ(noise,wx,wz,180)`; `b = fbm2(q0*0.0038,q1*0.0038,4)*0.5+0.5`; `v = fbm2(wx*0.004+9,wz*0.004,2)*0.5+0.5`; `m = max(0,(v-0.48)/0.19)`; `h = SEA-1 + (b-0.5)*10*rugged + sin(wx*0.013 + wz*0.021)*1.6 + pow(m,1.5)*20*rugged` |
| `shatter` | `q = warpXZ(noise,wx,wz,90)`; `ridge = 1 - abs(fbm2(q0*0.009+17, q1*0.009,4))`; `v = pow(ridge,1.4)*40*rugged`; `v = round(v/7)*7` (cliff quantization); `h = SEA-6 + v + fbm2(wx*0.05,wz*0.05,3)*2` |
| `hive` | `b = fbm2(wx*0.004,wz*0.004,4)*0.5+0.5`; `h = SEA-2 + (b-0.5)*12*rugged + hexDome(wx,wz,34)*(0.8 + wet*0.5)` |
| `alien` | `q = warpXZ(noise,wx,wz,160)`; `b = fbm2(q0*0.0055,q1*0.0055,5)`; `spire = pow(max(0, fbm2(q0*0.012,q1*0.012,4)-0.45), 1.6)`; `h = SEA-6 + (b*0.5+0.5)*18*rugged + spire*44*rugged` |
| `continental` (default) | `q = warpXZ(noise,wx,wz,190)`; `b = fbm2(q0*0.005,q1*0.005,5)`; `h = SEA-5 + (b*0.5+0.5)*30*rugged`; `+ fbm2(wx*0.05,wz*0.05,3)*3.5` |

Then **always**:
```
h += fbm2(wx*0.0028, wz*0.0028, 2) * CONTINENT_AMP[T] * rugged
return clamp(3, WORLD_H-8 /*88*/, h | 0)       // int truncation (h|0), clamped 3..88
```

### 4.0 `computeCharAxes()` — planet personality

```javascript
rnd = mulberry32((seed ^ 0xA45C1) >>> 0)
rugged = 0.72 + rnd()*0.56        // 0.72 .. 1.28  (amplitude/frequency multiplier)
temp   = rnd()                    // 0..1 (sub-biome cold/warm axis)
wet    = rnd()                    // 0..1 (sub-biome dry/wet axis)
```

### 4.2 Sub-biomes (`subBiomeAt`/`subDefAt`)

```
subBiomeAt(wx,wz):  m = fbm2(wx*0.0016 + temp*91, wz*0.0016 - wet*57, 3)*0.5+0.5
                    return clamp(0, sub.length-1, (m*sub.length)|0)
subDefAt:           returns biome.sub[subBiomeAt] or null
```
Each sub entry is `{ t?: treeMul, f?: flowerMul, g?: groundBlock }`. `g` overrides the surface block for that sub-band (affects surface color/vegetation only, not height).

### 4.3 Terrain operators (exact)

```javascript
warpXZ(n, wx, wz, amount):
  return [ wx + n.fbm2(wx*0.0021 + 7.3, wz*0.0021 - 2.1, 3) * amount,
           wz + n.fbm2(wx*0.0021 - 3.7, wz*0.0021 + 9.1, 3) * amount ]

craterField(wx, wz, opts):
  cell = opts.cell; cx = floor(wx/cell); cz = floor(wz/cell)
  rnd = hash2(cx, cz, 0xCEA7)
  if rnd() > (opts.chance||0.5): return 0
  dx = (wx - (cx + 0.15 + rnd()*0.7) * cell) / cell
  dz = (wz - (cz + 0.15 + rnd()*0.7) * cell) / cell
  r0 = (opts.r0||0.18) * (0.6 + rnd()*0.8)
  r1 = (opts.r1||0.42) * (0.6 + rnd()*0.8)
  d = hypot(dx, dz)
  if d > r1: return 0
  if d < r0: return -(opts.floor||12) * (1 - (d/r0)*0.25)      // pit (deeper toward center)
  t = (d - r0) / max(1e-4, r1 - r0)
  return (opts.rim||9) * sin(t * PI)                            // rim bulge

spireField(n, wx, wz, freq, th, gain, cap):
  b = n.fbm2(wx*freq, wz*freq, 4)
  m = max(0, b - th)
  return min(cap, m*m*gain)

hexDome(wx, wz, cell):                    // hexagonal dome lattice (hive)
  q = (0.577350269*wx - wz/3) / cell
  r = (0.666666667*wz) / cell
  hq = round(q); hr = round(r)
  d = max(|q-hq|, |r-hr|, |(q-hq)+(r-hr)|)     // hex distance to cell center
  b = max(0, 1 - d*1.35)
  return b*b*18

floatIslandAt(n, wx, wz):                // alien floating island
  mask = n.fbm2(wx*0.0045, wz*0.0045, 4)*0.5 + 0.5
  if mask < 0.62 || mask > 0.78: return null
  body = n.fbm2(wx*0.011 + 31, wz*0.011 - 17, 4)*0.5 + 0.5
  thick = max(0, body - 0.35)*14 + 3
  return { base: (54 + mask*20)|0, thick: thick|0 }
```

### 4.4 Caves (`isCave(wx,y,wz)`), selected by `biome.terrain.caves`

| Type | Threshold (returns true when) |
|---|---|
| `lava_tubes` | `a = vnoise3(wx*0.07, y*0.11, wz*0.07, 0xCAFE11)`; `b = vnoise3(..., 0xCAFE12)`; `abs(a-0.5)<0.07 && abs(b-0.5)<0.07` **OR** `c = vnoise3(wx*0.03,y*0.06,wz*0.03,0xCAFE13) > 0.88` |
| `ice` | `a,b = vnoise3(wx*0.04, y*0.06, wz*0.04, salt 0xCAFE21 / 0xCAFE22)`; `abs(a-0.5)<0.055 && abs(b-0.5)<0.055` |
| `geodes` | cell `26`; `gx=floor(wx/26), gz=floor(wz/26)`; `rnd=hash2(gx,gz,0x6E0D)`; `if rnd()>0.55 return false`; center `(gx*26+13, cy, gz*26+13)` with `cy = 18 + rnd()*26`; `rad = 3 + rnd()*4`; return sphere test `dx²+dy²+dz² < rad²` (genChunk lines walls with crystal at 12%) |
| `swamp_caves` | `vnoise3(wx*0.05, y*0.09, wz*0.05, 0xCAFE31) > 0.8` |
| `standard` (default) | `a,b = vnoise3(wx*0.045, y*0.075, wz*0.045, 0xCAFE01/0xCAFE02)`; `abs(a-0.5)<0.05 && abs(b-0.5)<0.05` **OR** `c = vnoise3(wx*0.024, y*0.045, wz*0.024, 0xCAFE03) > 0.855` |

### 4.5 Ore veins — exact table

```javascript
rng = hash2(cx, cz, 0x0DE5)              // per-chunk RNG
ores = [
  { id: coal_ore(id 7),     exp: 0.7,  size: 8, yMin: 4, yMax: 40 },
  { id: iron_ore(id 8),     exp: 0.62, size: 7, yMin: 3, yMax: 34 },
  { id: copper_ore(id 9),   exp: 0.62, size: 7, yMin: 3, yMax: 34 },
  { id: titanium_ore(id 10),exp: 0.26, size: 5, yMin: 2, yMax: 20 },
  { id: gold_ore(id 12),    exp: 0.17, size: 4, yMin: 2, yMax: 16 },
  { id: uranium_ore(id 11), exp: 0.11, size: 4, yMin: 2, yMax: 12 },
]
for each ore:
  expc = ore.exp * biome.oreMul
  n = floor(expc) + (rng() < (expc % 1) ? 1 : 0)     // stochastic rounding of expected count
  while n-- > 0:
    lx = rng()*16|0;  lz = rng()*16|0
    y  = ore.yMin + (rng()*(ore.yMax - ore.yMin))|0
    veinSize = 3 + (rng()*ore.size)|0
    for v in 0..veinSize:
      if in-chunk && 0<y<96:
        cur = data[lidx(lx,y,lz)]
        if cur==stone || cur==deep: data[lidx(lx,y,lz)] = ore.id
      lx += rng()*3-1 |0;  y += rng()*3-1 |0;  lz += rng()*3-1 |0    // random walk ±1 per axis
```
`biome.oreMul` per biome: lush 1.0, desert 1.3, frozen 1.2, **volcanic 2.0**, alien 1.5, ocean 0.9, crystal 1.4, fungal 1.2, ashen 1.8, amber 1.1, ferrous 1.6, murk 1.0, salt 1.0, obsidian 1.7, redmoss 1.15, hive 1.3.

### 4.6 `decorColumn` — biome signature surface decorations (column RNG `cr`)

| `biome.key` | Rule |
|---|---|
| `desert`, `amber` | if `cr()<0.006`: pillar of `n=1+(cr()*3|0)` blocks above surface; amber uses `amber` for the first block, else `stone` |
| `frozen` | if `cr()<0.007`: `n=1+(cr()*3|0)`; blocks `i=1..n`, top block `crystal`, lower `ice` |
| `volcanic`,`obsidian`,`ferrous` | if `cr()<0.008`: `n=1+(cr()*3|0)` basalt spires |
| `ashen` | if `cr()<0.01`: `log` at h+1, plus 40% chance `log` at h+2 (charred stump) |
| `salt` | if `cr()<0.006`: `n=1+(cr()*2|0)` `salt` pillar |
| `murk` | if `cr()<0.05`: `glow_shroom` at h+1 |
| `redmoss` | if `cr()<0.005`: `n=1+(cr()*2|0)` `stone` ridge |
| `crystal` | if `cr()<0.008`: `n=2+(cr()*4|0)` `crystal` spire |

### 4.7 Trees / giant mushrooms

`treeAt(wx,wz)`:
```
r = hash2(wx,wz, 0xABCD)
mul = (sd && sd.t !== undefined) ? sd.t : 1
if r() >= biome.trees * mul: return null
h = heightAt(wx,wz)
if h <= SEA + (biome.seaLift||0): return null      // no trees at/below sea
return { h, th: 4 + (r()*3)|0, rng: r }            // trunk height 4..6
```
`biome.trees`: lush 0.012, desert 0.001, frozen 0.004, volcanic 0, alien 0.008, ocean 0.007, crystal 0, fungal 0.010, ashen 0, amber 0.001, ferrous 0, murk 0.004, salt 0, obsidian 0, redmoss 0.003, hive 0.

**Normal tree** (non-mushroom biome):
- Trunk: `log` at `(lx, h+y, lz)` for `y = 1..th` (if in-chunk and `h+y<96`); plus one `leaves` at `h+th+2` (top cap).
- Canopy: for `ly in th-1..th+1`, `ox in -2..2`, `oz in -2..2`: `dist = |ox|+|oz|+|ly-th|`; skip if `dist > 3 || tr() < 0.15`; place `leaves` at `(lx+ox, h+ly, lz+oz)` only if in-chunk, `<96`, and target cell is air (0).

**Giant mushroom** (`biome.mushroom`, i.e. fungal/murk):
- Stem: `mush_stem` at `(lx, h+y, lz)` for `y = 1..th`.
- Cap: at `ty = h+th+1`, for `ox,oz in -2..2` skipping the 4 corners (`|ox|==2 && |oz|==2`), place `mush_cap` if air.
- Top center: `mush_cap` at `(lx, h+th+2, lz)`.

### 4.8 Structures

`genStructures()` (seed-derived, deterministic): `rnd = mulberry32((seed ^ 0x57A7C7)>>>0)`; `SEAB = SEA + seaLift`; `onLand(x,z) = heightAt(x,z) > SEAB + 1`.

- **Habitable** (`!biome.haz`): `want=3` villages, ≤70 tries each. Candidate center `x = (rnd()*1300|0)-650`, `z = (rnd()*440|0)-220`; must be on land and ≥240 blocks from any prior structure. `n = 4+(rnd()*3|0)` huts at angle `i/n*2π + rnd()*0.7`, distance `8 + rnd()*7`; keep only huts that are on land; require ≥3 valid huts. Stored as `{type:'village', x, z, kind:0, name:'拓荒者村落', huts, h}`.
- **Hazardous** (`biome.haz`): `want=3` ruins, ≤70 tries, same position range, ≥220-block separation, `kind = rnd()*3|0`; names `['先民石环','哨戒方尖碑','崩塌回廊']`; stored `{type:'ruin', x, z, kind, name, h, seed:(rnd()*0xFFFF|0)}`.

**`stampHut`** (5×5 hut, `s=2`, floor `f = hut.h+1`):
- Foundation/floor: fill `y in [min(gh, f-1) .. f-1)` with `biome.dirt`; `planks` at `f-1`.
- Interior cleared: air at `y in f..f+4` (also removes terrain/trees).
- Edge (`|dx|==s || |dz|==s`): walls at `y in f..f+2`; corner block = `log`, edge = `planks`; door (clear `f`,`f+1`) at `dx==0 && dz==s`; window (`glass` at `f+1`) where `(|dx|==s && dz==0) || (dx==0 && dz==-s)`.
- Roof: `planks` at `f+3` over full 5×5.

**`stampVillageCenter`**: `log` at `h+1..h+2`, `lamp` at `h+3` at center.

**`stampRuin`** (`R=10`, per-column `hr = hash2(wx,wz,st.seed)`):
- kind 0 (stone circle): ring at `|dist-7|<0.7` with `hr()<0.7` → pillar of `hh = 2+(hash2(wx,wz,seed+7)()*3|0)` blocks (top `deep`, body `stone`) standing at `gh+1..gh+hh`; center (`dist<1.6`): `stone` at `gh+1`, `lamp` at `gh+2` (only exact center `dx==dz==0`).
- kind 1 (obelisk, 3-step taper), `t0 = st.h`: within `|dx|<=1 && |dz|<=1`: `deep` at `t0+1..t0+3`; if `|dx|+|dz|<=1`: `stone` at `t0+4..t0+8`; center column: `deep` at `t0+9..t0+12`, `lamp` at `t0+13`.
- kind 2 (collapsed corridor): interior rect `|dx|<=8, |dz|<=6`; edge = `(|dx|==8 && inZ) || (|dz|==6 && inX)` → broken wall of `hh = hr()*4|0` (0..3) blocks at `gh+1..gh+hh`, each block `hr()<0.25 ? deep : stone`; non-edge interior with `hr()<0.3` → `stone` floor tile at `gh`.

`stampStructures` iterates all structures; for villages stamps each hut + center, for ruins stamps ruin. Each stamper early-outs if the stamp box is outside the chunk bounds.

---

## 5. Block-setting API and edit propagation

```javascript
get(x,y,z)     // floors coords; y out of [0,96) → 0; missing chunk → 0; else data[lidx]
getDef(x,y,z)  // BLOCK_BY_ID[get(...)] || BLOCKS.air
set(x,y,z,id,silent)
inBounds(x,y,z) // y in [0,96)
```
`set` algorithm:
1. Floor coords; `if y<0 || y>=96 return`.
2. `cx=cf(x), cz=cf(z)`; `c = chunks.get(ckey)` else `c = genChunk(cx,cz)` (edits force-generate).
3. `c.data[lidx] = id`; `c.modified = true`; `c.needSave = true`.
4. If `!silent`: `markDirty(c)`; then mark neighbor chunks dirty when the edit is on a chunk border:
   - `lx==0 → mark(cx-1,cz)`; `lx==15 → mark(cx+1,cz)`; `lz==0 → mark(cx,cz-1)`; `lz==15 → mark(cx,cz+1)`.

`markDirty(c)`: `c.dirty = true; streamDirty = true`.

- There is **no `damage` function in `world.js`**; mining/durability is handled by callers (`player.js`/`main.js`) via `get`/`set`. For a 1:1 port you only need `get/getDef/set/inBounds`.
- **Mesh rebuild triggers**: a chunk's mesh is rebuilt by `stream()` when `c.dirty && r <= MESH_R` and there is mesh budget. `dirty` is set by (a) edit `markDirty`, (b) neighbor-chunk border edits, (c) chunk generation (`markNeighborsDirty`), (d) mesh unload (`markDirty` on return). The `streamDirty` global gates the whole scan.

---

## 6. Water / lava

- Water block: **id 16** (`BLOCKS.water`): `{ solid:false, transparent:true, liquid:true, tiles:{all:'water'} }`.
- Lava is the same block id 16; appearance differs only via `biome.waterTint` (`volcanic 0xff6a1a` orange, `obsidian 0x4a3a6a`). `biome.dry` suppresses water; `biome.lava` (volcanic only) re-enables liquid in a dry biome.
- Water **meshing** (in `buildChunkMesh`):
  - Water is written to `wpos/wnor/wuv/wcol/wind` (separate water geometry) → `c.waterMesh`.
  - Face-culling for liquid: skip face if neighbor `id == water` (internal faces), **or** neighbor `solid && !transparent` (hidden against opaque terrain). Faces against transparent/air are drawn.
  - Top vertices (`cnr[1]==1`) are lowered by `0.12` (water surface sits slightly below cell top).
  - Vertex color: `sh = 0.72 + face.shade*0.28`, multiplied by `waterTintRGB()` = `[r,g,b]/255` from `biome.waterTint`.
  - `waterMat` opacity 0.72, `renderOrder = 1` (drawn after solid so the lakebed is always under the surface).
- **Wave animation** (vertex shader, injected for `waterMat`):
```glsl
// only for upward-facing vertices (normal.y > 0.5):
transformed.y += sin(transformed.x * 0.85 + uWTime * 2.2) * 0.035
               + cos(transformed.z * 0.70 + uWTime * 1.6) * 0.035;
```
`uWTime` advances by `dt` every `update()` (`waterWaveU.t.value += dt`). Side walls stay static (normal.y ≤ 0.5). Two sine/cosine terms, amplitude 0.035 each, spatial frequencies 0.85 (x) and 0.70 (z), angular speeds 2.2 and 1.6 rad/s.

---

## 7. Far / planet-curvature shader trick (exact)

### 7.1 Uniforms (`curveU`)

```
amt  (float, default 0)     uCurveAmt
cx   (float, default 0)     uCurveCX     — curvature center X (player/ship pos)
cz   (float, default 0)     uCurveCZ
fade (float, default 1)     uCurveFade   — global alpha fade (space entry dissolve)
grow (float, default 1)     uCurveGrow   — vertical squash/stretch about anchor
edgeR(float, default 9999)  uCurveEdge   — radial edge-fade radius (blocks)
uGrowY = SEA_Y = 28         (growth anchor)
```

### 7.2 Vertex displacement (exact GLSL, injected after `#include <begin_vertex>`)

```glsl
{
  transformed.y = uGrowY + (transformed.y - uGrowY) * uCurveGrow;   // anchor = 28
  float _cdx = transformed.x - uCurveCX;
  float _cdz = transformed.z - uCurveCZ;
  vEdgeR2 = _cdx * _cdx + _cdz * _cdz;
  vScanXZ = transformed.xz;
  transformed.y -= uCurveAmt * (_cdx * _cdx + _cdz * _cdz) * 0.002;  // paraboloid bend
}
```
- Displacement = `uCurveAmt * (dx² + dz²) * 0.002`, subtracted from Y. Curvature radius = `1/(2·0.002) = 250` blocks at `uCurveAmt=1` (matches the `0.004` curvature comment).
- The grow term compresses/expands Y about `SEA_Y=28` (used during atmo/space handoff).

### 7.3 Fragment injection (after `#include <fog_fragment>`)

```glsl
gl_FragColor.a *= uCurveFade * smoothstep(0.0, 3600.0, uCurveEdge * uCurveEdge - vEdgeR2);
// scan pulse:
{
  float _sd = length(vScanXZ - vec2(uScanCX, uScanCZ)) - uScanR;
  float _bk = -_sd;
  float _trail = smoothstep(0.0, 6.0, _bk) * (1.0 - smoothstep(10.0, 55.0, _bk));
  vec2 _gv = abs(fract(vScanXZ * 0.125) - 0.5);
  float _grid = smoothstep(0.40, 0.5, max(_gv.x, _gv.y));
  gl_FragColor.rgb += vec3(0.13, 0.86, 0.9) * (exp(-_sd * _sd * 0.018) + _grid * _trail * 0.5) * uScanA;
}
if (gl_FragColor.a < 0.04) discard;
```

**Edge fade**: alpha is multiplied by `smoothstep(0, 3600, edgeR² - r²)`. With `edgeR=9999` (default), `edgeR²≈9.998e7` so the smoothstep is ≈1 (no fade). With `edgeR=160` (space preview), the terrain fades to transparent beyond a 160-block radius circle. The `smoothstep(0, 3600, ...)` window makes the fade span ~`3600/(2·edgeR)` blocks.

**Scan pulse** (`uScanR/uScanCX/uScanCZ/uScanA`, driven by `setScanPulse(r,cx,cz,a)`):
- Ring: cyan `vec3(0.13,0.86,0.9)`, intensity `exp(-sd²*0.018)` — Gaussian ring at `sd=0` (σ ≈ 1/√(2·0.018) ≈ 5.27 blocks).
- Trailing holographic grid: `_trail` is nonzero for `_bk ∈ (0,55)` i.e. `sd ∈ (-55, 0)` (inside the ring, behind the front), peaking around `_bk≈6..10`; `_grid` = line grid with spacing `1/0.125 = 8` blocks (`fract(vScanXZ*0.125)`), lines smoothed `smoothstep(0.40,0.5,...)`. Trail contribution = `_grid*_trail*0.5`.
- Total scan add = `cyan * (ring + grid*trail*0.5) * uScanA`. `main.js:4230` drives it with `r = atmoScanFx.t * 480`, `a = fadeIn*fadeOut*0.9`.

### 7.4 How `uCurveAmt/cx/cz/grow/fade` are animated (driver is `main.js`, not `world.js`)

`world.js` only exposes `setCurve(amt, cx, cz, fade, grow, edgeR)`. The caller drives it:
- **In atmosphere/landing** (`main.js:3159-3161`): `curveAmt = clamp((camera.y - 62) / (HANDOFF_Y - 62), 0, 1)` where `HANDOFF_Y = 150`, i.e. `curveAmt = clamp((camera.y - 62)/88, 0, 1)`; `cx=cz = camera/ship position`; `fade=grow=1, edgeR=9999`.
  - Flat at `camera.y=62`, fully spherical at `camera.y=150`.
- **Space surface preview** (`main.js:1865`): `setCurve(1, v.x, v.z, fade, grow, 160)` where `fade = clamp((340-bestD)/50, 0, 1)`, `grow = clamp((330-bestD)/165, 0.08, 1)`, `edgeR=160` (radial circular fade).

### 7.5 Far simulated terrain (`farMesh`)

- Single `129×129`-vertex grid (`FAR_N=129`), cell step `FAR_STEP=12` blocks (adjustable). One `BufferGeometry` with `position` + `color` + `normal` (normals recomputed after a full refresh), index = two triangles per cell.
- Material `MeshLambertMaterial({vertexColors:true, transparent:true, depthWrite:false})`, `frustumCulled=false`, `renderOrder=-1`, hidden until first full refresh.
- `tickFar(cx,cz)` snaps the player to a 64-block grid; when the snap changes it re-runs the grid, 10 rows per call. Each vertex: `position = (wx, mapHeightAt(wx,wz) - 2.2, wz)` (the −2.2 sink keeps real chunks on top), `color = mapColorRGB(wx,wz)/255`.
- **Far hole** (so the real chunks are not covered by the fake surface): fragment `gl_FragColor.a *= smoothstep(uFarR0, uFarR1, vEdgeR2)` with `uFarR0 = r0², uFarR1 = r1²` from `farHoleU` (§1.2). This carves a radial hole centered on the player's curvature center, radius linked to `MESH_R`.
- The far mesh also gets `applyCurve` (same vertex bend), so it conforms to the planet sphere at altitude.

---

## 8. Lighting / AO / face culling / atlas

### 8.1 There is **no ambient-occlusion pass**

Vertex lighting is per-face directional bake only, encoded in vertex colors and multiplied by the block albedo (texture). There is no voxel AO, no smooth lighting, no light propagation. Point lights (`lampPool`) and the scene's `DirectionalLight`/`AmbientLight`/`HemisphereLight` (Lambert material) provide runtime lighting. **For a 1:1 port, replicate only the face shade constants below.**

### 8.2 Face table (`FACES`) and baked shade

```javascript
FACES = [
  { dir:[ 1,0,0], shade:0.80 },   // +X
  { dir:[-1,0,0], shade:0.80 },   // -X
  { dir:[ 0,1,0], shade:1.00 },   // +Y (top)
  { dir:[ 0,-1,0], shade:0.50 },  // -Y (bottom)
  { dir:[ 0,0,1], shade:0.65 },   // +Z (front)
  { dir:[ 0,0,-1], shade:0.65 },  // -Z
];
```
For solid blocks the vertex color is `shade * (def.glow ? 2.2 : 1)` per RGB channel. Glow blocks are "full bright" (2.2×).

### 8.3 Face culling (only exposed faces are emitted)

Per cell, per face, `nDef = getDef(x+dir.x, y+dir.y, z+dir.z)`:

- **Liquid** (`def.liquid`): skip face if `nDef.id === def.id` OR (`nDef.solid && !nDef.transparent`).
- **Low-box** (`def.lowbox` numeric and `!def.machine`, e.g. `slab` lowbox 0.45): skip only bottom face (`f===3`) if `nDef.solid && !nDef.transparent`; all other faces always drawn.
- **Regular solid/transparent**: skip face if `nDef.solid && !nDef.transparent && !nDef.cross && !nDef.machine`; additionally skip if `nDef.id === def.id && def.transparent && !def.fancy` (same-block transparent neighbors such as glass/leaves do not cull each other unless `fancy`).
- `def.machine` blocks are skipped entirely (never meshed; rendered by `factory.js`).
- `def.cross` blocks (plants) are rendered as two full-height diagonal quads, not as cubes.

### 8.4 Texture atlas / UV layout

- Atlas: single canvas texture, `TS = 16` px tiles, `COLS = 16` columns → 256×256 px, 256 tiles. `magFilter = NearestFilter`, `minFilter = NearestMipmapNearestFilter`, mipmaps on (pixel-art look).
- Tile index `i` assigned sequentially by `tile(name,...)`. Tile origin at `ox = (i%16)*16`, `oy = (i/16|0)*16` (row-major, top-down canvas).
- `uvRect(name)` returns the UV rect for a tile:
```
u  = (i % 16) / 16
v  = 1 - ((i/16|0) + 1) / 16     // flip V (canvas top-down → GL bottom-up)
u0 = u, v0 = v, u1 = u + 1/16, v1 = v + 1/16
```
- `tileFor(def, faceIndex)` picks the tile name per face:
```
if t.all && !t.top && !t.front → t.all
face 2 (top)     → t.top    || t.all || t.side
face 3 (bottom)  → t.bottom || t.all || t.side
face 4 (front,+Z) && t.front → t.front
else             → t.side   || t.all || t.top
```
- **UV emission for a cube face** (4 corners, matching `FACES` corner order): `[u0,v1, u0,v0, u1,v1, u1,v0]` (i.e. corners 0..3 → bottom-left, top-left, bottom-right, top-right in UV space). Cross-block quads use `[u0,v0, u1,v0, u1,v1, u0,v1]`.
- Per-face corner ordering follows `FACES[].corners` (4 `[x,y,z]` each); indices `[b, b+1, b+2, b+2, b+1, b+3]` per face (two triangles), double-sided.

---

## 9. Performance tricks, streaming, budget

- **No greedy meshing.** Every exposed face is emitted individually; chunks are simple indexed `BufferGeometry` with interleaved attribute arrays (`position/normal/uv/color/aEm/index`).
- **Streaming** (`stream(px,pz)`) with per-frame budgets:
  - `genBudget = 4` chunks, `meshBudget = 2` meshes per frame.
  - Iterates Chebyshev rings `r = 0..GEN_R` from inside-out. A chunk at ring `r` is generated if missing (consuming genBudget) and meshed if `r <= MESH_R && dirty` (consuming meshBudget).
  - **Border pre-fetch**: before meshing a chunk, all 4 neighbors must exist; missing neighbors are generated *within the same genBudget* (up to 4 total/frame). If budget runs out, the chunk is skipped this frame (no holes — it meshes next frame once neighbors exist).
  - **Idle gate**: if `!streamDirty` and player chunk unchanged since last scan, the entire function returns immediately (avoids thousands of Map lookups/frame when stationary).
- **Unload** (keep data, drop meshes): any chunk with a mesh and Chebyshev distance `> UNLOAD_R` has its mesh/waterMesh disposed; `markDirty` so it rebuilds on return.
- **Data eviction**: chunks with **no mesh**, Chebyshev distance `> UNLOAD_R + 6`, **not modified**, and **not pinned** (by a `Factory` machine) are deleted from the Map. Unmodified chunks regenerate deterministically (or from the v4 snapshot), so no data loss. Modified chunks are kept in memory forever (they flow to save/network).
- **Mesh rebuild** is triggered only via the `dirty` flag (see §5). `boundingSphere.radius += 60` per mesh to account for the curvature vertex displacement (prevents frustum culling artifacts).
- **Load-screen pregen** (`pregen(wx,wz,radius,progressCb)`): generates the `(2r+1)²` square, then meshes the inner `(2r-1)²`, yielding to the event loop (`setTimeout 0`) each row; progress callback reports 0..0.5 (gen) and 0.5..1.0 (mesh).
- **Lamp pool**: constant 6 point lights (no shader recompiles); every 0.5 s the nearest 6 glow blocks (within 60² blocks) drive positions/colors/intensities (0.95) of the pool; unused lamps set intensity 0.
- **Fade-in** (`startFadeIn`/`tickFade`): freshly meshed chunks fade over 0.9 s. Solid terrain uses a *brightness* fade (clone material, `color` 0→1, alpha stays 1, so `alphaTest` doesn't cull whole chunks); water uses *opacity* fade (0→0.72). Cloned materials re-inject `applyCurve` (+ `applyGlow`/`applyWaterWaves`) because `clone()` doesn't carry `onBeforeCompile`.
- **Far mesh**: single fake-surface grid (§7.5) to extend the horizon without generating chunks; depthWrite off, renderOrder −1, frustum-culled off.

---

## 10. World save format

### 10.1 What is saved

Two independent mechanisms:

1. **Modified chunks only** (`serialize()`): `{ seed, mods }` where `mods = { "cx,cz": rleArray }` and `rleArray` is the flat `[run,id,run,id,...]` JS array from `rleEncode`. Only `c.modified` chunks are included. `serializeChunk(cx,cz)` returns the same RLE for one modified chunk (network incremental sync); `chunkModified(cx,cz)` reports whether it is modified.

2. **Full chunk snapshots (v4, Minecraft-style)** — `takePendingSave(max)` returns up to `max` records `{ cx, cz, mod: boolean, rle: Uint16Array }`:
   - Every procedurally-generated chunk sets `needSave=true` and is written as a full snapshot (so generator upgrades never corrupt old worlds).
   - `rle` = `rleEncode16(c.data)` — a `Uint16Array` of flat `[run, id]` pairs, run ≤ 65535 (same format as `rleEncode`).
   - Modified chunks are prioritized in the queue; `mod` mirrors `c.modified`. After dequeuing, `needSave=false`.
   - `requeueSave(list)` re-marks failed uploads; `pendingSaveCount()` counts; `chunkIsSaved(cx,cz)` = `c.fromSave`.

### 10.2 RLE format detail

- Flat sequence of unsigned 16-bit values: `run0, id0, run1, id1, ...`, ending at exactly `CHUNK_CELLS` total decoded cells.
- Decode validation: sum of even-indexed values must equal `data.length` (24576) or the record is rejected as corrupt (also catches world-height changes).
- `seed` is stored in the save header (not per chunk); `init(biomeKey, worldSeed, mods, savedChunksMap)` restores it and recomputes `charAxes`/structures from it.

### 10.3 `init` / `dispose`

`init(biomeKey, worldSeed, mods, savedChunksMap)`: disposes prior state, sets `seed`, looks up `biome = BIOMES[biomeKey]` (with `biome.key = biomeKey`), `noise = makeNoise(worldSeed)`, `computeCharAxes()`, stores `savedMods`/`savedChunks`, resets `chunks`/`group`/`streamDirty`, `genStructures()`, builds the 6-light pool, resets far state. `dispose()` removes/disposes all meshes and resets Maps/flags.

---

## Block ID table (for reference; from `data.js`)

| id | key | id | key | id | key |
|---|---|---|---|---|---|
| 0 | air | 20 | ice | 40 | burner (machine) |
| 1 | grass | 21 | snow | 41 | crystal |
| 2 | dirt | 22 | basalt | 42 | mush_stem |
| 3 | stone | 23 | alien | 43 | mush_cap |
| 4 | sand | 24 | barrier (bedrock) | 44 | ash |
| 5 | log | 30 | furnace (m) | 45 | amber |
| 6 | leaves | 31 | miner (m) | 46 | rust |
| 7 | coal_ore | 32 | belt (m, lowbox) | 47 | salt |
| 8 | iron_ore | 33 | assembler (m) | 48 | obsidian |
| 9 | copper_ore | 34 | solar (m, lowbox) | 49 | redmoss |
| 10 | titanium_ore | 35 | refinery (m) | 50 | hive |
| 11 | uranium_ore | 36 | chest (m) | 51 | murk |
| 12 | gold_ore | 37 | reactor (m) | 52 | glow_shroom (cross, glow) |
| 13 | sodium_plant (cross) | 38 | launchpad (m, lowbox) | 53 | beacon (m) |
| 14 | oxygen_plant (cross) | 39 | wind (m) | 54 | lumberbot (m) |
| 15 | fern (cross) | | | 55 | collector (m) |
| 16 | water (liquid) | | | 56 | medbay (m) |
| 17 | planks | | | 57 | slab (lowbox 0.45) |
| 18 | glass (transparent) | | | 58 | metal |
| 19 | lamp (glow) | | | 59 | concrete |

Block flags used by world gen/mesh: `solid` (default true), `transparent`, `liquid`, `cross`, `machine`, `lowbox` (bool or number), `glow`, `fancy`, `ore`, `tiles` (`{all}` or `{top,side,bottom}` or `{all,front}`).

## Biome → terrain/type/palette/hazard (16 biomes)

| key | terrain type | caves | grass/dirt/deep | waterTint | dry/lava | seaLift | trees | flowers | oreMul | haz |
|---|---|---|---|---|---|---|---|---|---|---|
| lush | continental | standard | grass/dirt/stone | 0x3e6bd6 | – | – | 0.012 | 0.02 | 1.0 | null |
| desert | dunes | standard | sand/sand/stone | 0x6db8c8 | – | – | 0.001 | 0.008 | 1.3 | heat |
| frozen | glacial | ice | snow/dirt/ice | 0x9fd4e8 | – | – | 0.004 | 0.006 | 1.2 | cold |
| volcanic | volcanic | lava_tubes | basalt/basalt/basalt | 0xff6a1a | dry+lava | – | 0.0 | 0.004 | 2.0 | heat |
| alien | alien | standard | alien/dirt/stone | 0x7a4ad8 | – | – | 0.008 | 0.03 | 1.5 | toxic |
| ocean | archipelago | standard | grass/sand/stone | 0x2b62c8 | – | 7 | 0.007 | 0.014 | 0.9 | null |
| crystal | glacial | geodes | snow/dirt/ice | 0x8fd8e8 | – | – | 0 | 0.004 | 1.4 | cold |
| fungal | continental | standard | alien/dirt/stone | 0x6a4a8a | – | – | 0.010 | 0.02 | 1.2 | toxic |
| ashen | flats | standard | ash/ash/basalt | 0x9a7a5a | – | – | 0 | 0.003 | 1.8 | rad |
| amber | dunes | standard | amber/sand/stone | 0xd8b048 | – | – | 0.001 | 0.006 | 1.1 | heat |
| ferrous | shatter | standard | rust/rust/basalt | 0x8a5a3a | – | – | 0 | 0.004 | 1.6 | storm |
| murk | swamp | swamp_caves | murk/dirt/stone | 0x2f7a5a | – | 4 | 0.004 | 0.035 | 1.0 | toxic |
| salt | flats | standard | salt/salt/stone | 0xcfe8f0 | – | – | 0 | 0.008 | 1.0 | null |
| obsidian | shatter | standard | obsidian/obsidian/basalt | 0x4a3a6a | dry | – | 0 | 0.002 | 1.7 | heat |
| redmoss | mesa | standard | redmoss/dirt/stone | 0xb06050 | – | – | 0.003 | 0.012 | 1.15 | cold |
| hive | hive | standard | hive/hive/stone | 0xd89830 | – | – | 0 | 0.01 | 1.3 | toxic |

(`mushroom` = fungal, murk. `crystals` = crystal (0.02). `flora` overrides: murk `[glow_shroom, glow_shroom, oxygen_plant]`, salt `[sodium_plant, sodium_plant, fern]`.)

---

## Key constants (bullet summary)

- `CHUNK = 16`, `WORLD_H = 96`, `SEA = 32`, `SEA_Y = 28`
- `CHUNK_CELLS = 24576`; live data = `Uint8Array`
- Chunk key (int) `= cx*65536 + cz`; string key `= "cx,cz"` (save boundary only)
- Index `= y*256 + lz*16 + lx`
- `GEN_R=17`, `MESH_R=16`, `UNLOAD_R=19`; `setViewDist(n)`: `GEN=n+1, UNLOAD=n+3`
- Curvature radius `= 250` blocks; displacement coefficient `0.002`; grow anchor `SEA_Y=28`
- `curveAmt = clamp((camY-62)/88, 0, 1)`; space preview `edgeR=160`, `grow` clamp to `[0.08,1]`
- Edge fade: `alpha *= smoothstep(0, 3600, edgeR² - r²)`; discard if `alpha < 0.04`
- Scan pulse color `(0.13,0.86,0.9)`, ring `exp(-sd²·0.018)`, grid spacing `8` blocks, trail window `_bk ∈ (0,55)`
- Face shades: `+Y 1.0, -Y 0.5, ±X 0.8, ±Z 0.65`; glow multiplier `2.2`; water shade `0.72 + shade·0.28`
- Water block id `16`; water opacity `0.72`; wave amp `0.035` (two terms: `x*0.85 @2.2`, `z*0.70 @1.6`); top inset `0.12`
- Noise: `fade(t)=t³(6t²-15t+10)`; `grad2 ∈ {x+y, -x+y, x-y, -x-y}`; `fbm2` default `oct=4, lac=2, gain=0.5`, normalized
- `rugged = 0.72 + rnd()*0.56`; height clamp `[3, 88]`
- Ore veins: coal `{0.7, size8, 4..40}`, iron/copper `{0.62, size7, 3..34}`, titanium `{0.26, size5, 2..20}`, gold `{0.17, size4, 2..16}`, uranium `{0.11, size4, 2..12}`; vein size `3 + rnd()*size`
- Caves salt seeds: standard `0xCAFE01/02/03`, lava_tubes `0xCAFE11/12/13`, ice `0xCAFE21/22`, swamp `0xCAFE31`, geodes `0x6E0D`
- Hash salts: crater `0xCEA7`, column decor `0x51CA`, ore `0x0DE5`, trees `0xABCD`, floating-island cap `0xF10A`, char axes `0xA45C1`, structures `0x57A7C7`, spawn `0xB00B5`
- Per-frame budget: `genBudget=4`, `meshBudget=2`; far refresh `10 rows/frame`; lamp scan `0.5s`, 6 lights, radius² `3600`
- Far mesh `129×129`, default step `12`, snap `64`; `setFarDist(d)` → `step = max(4, round(d/64))`
- RLE run ≤ `65535`, stored as flat `Uint16Array` pairs; decode validates total `== 24576`
- Structure placement ranges: x `[-650,650]`, z `[-220,220]`; village spacing `240`, ruin spacing `220`

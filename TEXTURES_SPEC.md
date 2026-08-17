# STARFORGE — Procedural Texture & Rendering Specification (Rust port reference)

> Repository note (2026-08-17): all legacy paths named below now live under `legacy-web/`.

Target crates: `image` (RGBA8 pixel buffer) + `bevy_render` / `wgpu` (samplers, textures, mipmaps).
This document is exhaustive for `js/textures.js` and the cross-file pieces it depends on. Every color
is given as an exact hex value; every algorithm as pseudocode. Where the JS reads a color as an
8-digit hex (`#RRGGBBAA`) or `rgba(...)`, the alpha is preserved.

---

## 0. Scope & source map (read this first)

`js/textures.js` contains **only** these four things:

1. `Tex` — the 16×16 block/texture atlas generator (Section 1–3).
2. `Icons` — 32×32 item icons (Section 4).
3. `StationTex` — 64×64 space-station tiles (Section 5).
4. `disposeObject3D` — a WebGL resource-release helper (not needed for a Rust port; skip it).

Things the task asked about that are **NOT** in `textures.js` (they live elsewhere and are documented
in later sections):

| Requested item | Actually lives in | Nature |
|---|---|---|
| Player/humanoid "skin texture" (9 customizable parts) | `js/humanoid.js`, `js/ui.js` | **Not a texture.** A parametric SVG-extruded 3D model with flat colors (Section 7). |
| Creature textures | `js/creatures.js` | **Not textures.** Box/GLB meshes with flat `MeshLambertMaterial` colors (Section 8). |
| Planet map (M key) | `js/main.js` | A 3D WebGL holographic globe, not a 2D texture (Section 6.5). |
| Galaxy/star/reticle textures | `js/ui.js` | Procedural canvases (Section 6). |
| Planet / sun / cloud textures | `js/space.js` | Procedural canvases (Section 6). |
| Pixel-art render mode (low-res + upscale) | `js/main.js`, `css/style.css` | Renderer settings + CSS (Section 10). |
| Crosshair | `index.html`, `css/style.css` | Pure CSS DOM element (Section 6.6). |
| Noise generator `makeNoise`/`fbm2` | `js/world.js` | Appendix A. |
| `mulberry32` RNG | `js/textures.js` (top) | Section 2. |

---

## 1. Texture atlas layout (`Tex`)

### 1.1 Dimensions & grid

```
const TS = 16;      // tile size in pixels (16×16)  — confirmed
const COLS = 16;    // tiles per row
canvas.width  = TS * COLS = 256
canvas.height = TS * COLS = 256
```

- Atlas canvas is **256×256 px**, holding a **16×16 grid** of **16×16 tiles** → up to **256 tile slots**.
- Tiles are packed **edge-to-edge with no margin, no padding, no gutter**.
- Tile `i` is assigned by a monotonically increasing `cursor` in **registration order** (Section 3 lists
  the exact order, which therefore defines every tile index).
- Tile `i` pixel origin (canvas top-left):
  ```
  ox = (i % COLS) * TS          // = (i % 16) * 16
  oy = floor(i / COLS) * TS     // = floor(i / 16) * 16
  ```
- A registry `index: { name -> i }` maps tile names to indices.

### 1.2 UV computation (exact)

`Tex.uvRect(name)`:

```
i  = index[name]
u0 = (i % COLS) / COLS                      // = (i % 16) / 16
v0 = 1 - (floor(i / COLS) + 1) / COLS       // = 1 - (floor(i/16) + 1)/16
u1 = u0 + 1 / COLS                          // = u0 + 1/16
v1 = v0 + 1 / COLS                          // = v0 + 1/16
// returns { u0, v0, u1, v1 }
```

Because the canvas is top-down (y grows downward) but GL textures are bottom-up, the V axis is
flipped: `v0` is the tile's **bottom** edge and `v1` its **top** edge in GL convention. In Rust with
`bevy_render`/`wgpu` (UV origin top-left of the image), you can either (a) keep the image top-left
origin and store the atlas rows directly, then use `v = (row)/16 … (row+1)/16` **without** the flip,
or (b) follow the JS exactly by pre-flipping the image and using the formula above. **Match the JS
semantics:** the v0/v1 above assumes the uploaded texture has been Y-flipped once (as three.js does
with `CanvasTexture`). If your mesh UVs are already bottom-up, replicate by flipping the atlas rows on
upload.

There is **no UV inset** (no half-texel or 1px padding). Correctness relies entirely on
**nearest-neighbor filtering** (below). If you later add padding to support bilinear, the UV formula
changes; the original does not pad.

### 1.3 Filtering / mipmap settings (the "最近 mip 采样" anti-flicker trick)

Exact three.js settings (same for the atlas texture, the extracted tile textures, the station tiles,
and the planet texture):

```
texture.magFilter      = NearestFilter              // GL_NEAREST            (0x2600)
texture.minFilter      = NearestMipmapNearestFilter // GL_NEAREST_MIPMAP_NEAREST (0x2700)
texture.generateMipmaps = true
```

In Rust / `bevy_render` / `wgpu` the equivalent `SamplerDescriptor` is:

```rust
SamplerDescriptor {
    mag_filter: FilterMode::Nearest,
    min_filter: FilterMode::Nearest,
    mipmap_filter: FilterMode::Nearest,  // <-- important: NOT the default Linear
    ..Default::default()                  // address modes per-section below
}
```

- **Magnification**: `Nearest` → hard, chunky pixel blocks up close.
- **Minification**: `NearestMipmapNearest` → pick the single nearest mip level (not trilinear), and
  inside that level still sample nearest. This is the anti-flicker/anti-moiré trick: at distance the
  GPU switches to a smaller mip (removing shimmer) while retaining the pixelated look.
- **Mipmaps must be generated** (`generateMipmaps = true`). The atlas is 256×256 and the extracted
  tiles are 16×16 — both power-of-two, so mip generation is valid.

### 1.4 Wrap modes & per-tile textures

- The **atlas texture itself** uses the default clamp-to-edge (no explicit wrap set in JS).
- `Tex.tileTexture(name, repeatX = 1, repeatY = 1)` extracts a single tile into its own **16×16**
  texture (used for machine materials and asteroids):
  - Copies the 16×16 sub-rect of the atlas into a new 16×16 canvas.
  - Same filters as above (`Nearest` / `NearestMipmapNearest`, mipmaps on).
  - `wrapS = wrapT = RepeatWrapping`, and `repeat.set(repeatX, repeatY)`.
- `Tex.tileCanvas(name)` returns the raw 16×16 canvas (used by item icons).

### 1.5 Face → texture mapping

The generator only produces the **pixels**. Which tile goes on which cube face is decided by block
definitions in `js/data.js` (not in `textures.js`): a block declares `tiles: { top, side, bottom,
front, all }`, and `Icons.get` uses `tiles.top`, `tiles.side`, `tiles.front` (see Section 4.2). For the
Rust port you need only reproduce the named tiles (Section 3) and then assign them per block in the same
way the game's `BLOCKS` table does. Cross-shaped plants (`cross: true`) render as two intersecting
quads using the single `side`/`all` tile.

---

## 2. RNG, pixel helpers, and the dithering convention

### 2.1 `mulberry32(seed)` — the deterministic PRNG

```
function mulberry32(seed) {
  let a = seed >>> 0;                       // uint32
  return function() {
    a |= 0;
    a = (a + 0x6D2B79F5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;   // -> float in [0,1)
  };
}
```

All operations are 32-bit wraparound; `Math.imul` is a 32-bit integer multiply. Returns a float in
`[0,1)`. **Reproduce this exactly** (including the multiply semantics) or tiles will differ.

### 2.2 Per-tile seeding

Each `tile(name, painter, seed)` uses:

```
rng = mulberry32(seed || (tileIndex * 7919 + 13))
```

So if no explicit seed is given, the seed is `index * 7919 + 13`. All speckle-based tiles rely on this
deterministic seed; the *speckle call order* therefore matters (identical order of `rng()` calls yields
identical pixels).

### 2.3 Pixel painter

```
makePX(ox, oy) -> (x, y, color) { fillRect(ox + x, oy + y, 1, 1); }
```
i.e. "paint a single 1×1 pixel at (x,y) inside the tile". In Rust: write one RGBA8 pixel into the tile
buffer.

### 2.4 `shade(hex, f)`

```
parse #RRGGBB; for each channel c in {r,g,b}:
  c' = clamp(round(c * f), 0, 255)
returns rgb(c',c',c')   // alpha untouched (full opacity)
```
`f` is a per-channel multiplier. Used by `ingotIcon`/`crystalIcon`/`chunkIcon` and the (unused) `pal`.

### 2.5 `speckle(px, rng, palette)` — the core dither/noise fill

```
for y in 0..TS:
  for x in 0..TS:
    px(x, y, palette[ floor(rng() * palette.length) ])
```

This is the **entire general "noise speckle" algorithm**: every pixel is independently chosen uniformly
from a small palette of 3–5 near-identical shades of one base color. There is no per-pixel noise field,
no hash of coordinates — just the seeded PRNG stream in row-major order.

### 2.6 Color palette / variance convention

- The `pal(base, n = 4, spread = 0.16)` helper encodes the intended dithering formula:
  ```
  palette[i] = shade(base, 1 - spread * i)   for i in 0..n-1
  ```
  i.e. each channel is multiplied by `1 - 0.16*i` → **16% lightness steps**, up to 4 shades. (Note: this
  helper is defined but the actual tiles below hardcode their palettes; those hardcoded palettes mostly
  follow the same ±8–16 % lightness-variation pattern.)
- Convention used throughout: **base color + a 3–5 color palette of lightness variants**, applied via
  `speckle`. Brighter/darker detail (cracks, highlights, edges) is layered on top with explicit 1px
  `px()` calls.

---

## 3. Block tiles — complete enumeration (registration order = tile index)

Legend for each entry: **name** `(index i)` — algorithm. `r` = the tile's `mulberry32` stream.
`speckle(px, r, [...])` means fill all 16×16 with uniform palette picks. `px(x,y,color)` writes one pixel.
`floor(r()*N)` yields an integer in `0..N-1` uniformly.

### Basic terrain

1. **grass_top** `(0)` — `speckle(px, r, ['#69b23f','#5da337','#74bd48','#619f3b','#7cc44f'])` (5 greens).

2. **dirt** `(1)` — `speckle(px, r, ['#8a5f3c','#7d5535','#95683f','#775033','#8a6039'])` (5 browns).

3. **grass_side** `(2)` —
   ```
   speckle(px, r, ['#8a5f3c','#7d5535','#95683f','#775033'])   // dirt base (4)
   for x in 0..16:
     h = 3 + floor(r()*2.4)                 // h ∈ {3,4,5}
     for y in 0..h: px(x, y, ['#69b23f','#5da337','#74bd48'][floor(r()*3)])
   ```
   → a wavy grass lip of height 3–5 px along the top edge.

4. **stone** `(3)` —
   ```
   speckle(px, r, ['#8c8c8c','#828282','#969696','#7a7a7a'])
   for i in 0..5: x=floor(r()*14); y=floor(r()*14); px(x,y,'#a3a3a3'); px(x+1,y,'#a3a3a3')
   ```
   → 5 light 1×2 crack dashes.

5. **sand** `(4)` — `speckle(px, r, ['#e0d29a','#d8c98e','#e8dba6','#d0c184'])`.

6. **gravel** `(5)` — `speckle(px, r, ['#8f8b87','#7c7975','#a09b96','#6e6a66','#95908b'])` (5 shades).

7. **log_side** `(6)` —
   ```
   for x in 0..16:
     band = ['#6b502f','#5e4629','#755834','#634a2b'][x % 4]
     for y in 0..16: px(x, y, r()<0.85 ? band : shade('#6b502f', 0.8 + r()*0.4))
   ```
   → vertical bark stripes repeating every 4 columns, 15 % random lighter/darker pixels.

8. **log_top** `(7)` —
   ```
   speckle(px, r, ['#b08d55','#a5854f'])
   for ring in [7,5,3,1]:                     // 7; >=1; -=2
     for a in 0..64:
       x = 8 + round(cos(a/64*2π) * ring * 0.9)
       y = 8 + round(sin(a/64*2π) * ring * 0.9)
       if in-bounds: px(x, y, '#8a6b3d')
   for i in 0..16:                            // full 1px border
     px(i,0,'#6b502f'); px(i,15,'#6b502f'); px(0,i,'#6b502f'); px(15,i,'#6b502f')
   ```
   → concentric growth rings + bark border.

9. **leaves** `(8)` — translucent, ~1/4 holes:
   ```
   pal = ['#3f7d2c','#357024','#488a33','#2e6420']
   for y in 0..16 for x in 0..16:
     if r()<0.24: continue                    // transparent hole (alpha = 0)
     px(x, y, pal[floor(r()*4)])
     if r()<0.06: px(x, y, '#5aa93f')         // occasional bright leaf tip
   ```
   **Alpha note**: skipped pixels must be fully transparent (RGBA `(0,0,0,0)`); the texture is rendered
   with transparency so you can see through the canopy.

10. **planks** `(9)` —
    ```
    speckle(px, r, ['#a8824f','#9d7948','#b28a55'])
    for y in (3; y<16; y+=4): for x in 0..16: px(x, y, '#7a5c35')   // y = 3,7,11,15
    px(4,1,'#7a5c35'); px(11,5,'#7a5c35'); px(2,9,'#7a5c35'); px(13,13,'#7a5c35')
    ```
    → horizontal plank seams every 4 rows + 4 "nail" dots.

11. **water** `(10)` — `speckle(px, r, ['#3e6bd6','#3862c7','#4675e0','#3455b8'])` (4 blues).
    **Static**; water motion is a vertex shader, not texture animation (Section 9).

12. **ice** `(11)` —
    ```
    speckle(px, r, ['#a8d4f0','#9ccbeb','#b6ddf5'])
    px(3,4,'#e0f2fc'); px(4,5,'#e0f2fc'); px(10,9,'#e0f2fc'); px(11,10,'#e0f2fc'); px(12,3,'#e0f2fc')
    ```

13. **snow_top** `(12)` — `speckle(px, r, ['#f2f6fa','#e8eef5','#fafcff','#e0e8f0'])`.

14. **snow_side** `(13)` —
    ```
    speckle(px, r, ['#8a5f3c','#7d5535','#95683f'])                 // dirt base (3)
    for x in 0..16: for y in 0..4: px(x, y, ['#f2f6fa','#e8eef5'][floor(r()*2)])
    ```
    → 4 px snow cap on top of dirt.

15. **basalt** `(14)` —
    ```
    speckle(px, r, ['#3a3a42','#33333a','#42424c','#2c2c33'])
    for i in 0..4: x=floor(r()*15); y=floor(r()*15); px(x,y,'#ff7733'); if r()<0.5: px(x+1,y,'#c94f1e')
    ```
    → dark volcanic rock + 4 glowing ember specks.

16. **alien_top** `(15)` — `speckle(px, r, ['#9a5fd0','#8b52c2','#a86ddb','#7d47b3','#b078e0'])`.

17. **alien_side** `(16)` —
    ```
    speckle(px, r, ['#6e4a8a','#61407c','#7b5498'])
    for x in 0..16: h = 3 + floor(r()*2.2); for y in 0..h: px(x, y, ['#9a5fd0','#a86ddb'][floor(r()*2)])
    ```

18. **barrier** `(17)` —
    ```
    speckle(px, r, ['#2a2a30','#222228','#32323a'])
    for i in 0..16: px(i,i,'#4a4a55'); px(15-i,i,'#4a4a55')        // X cross
    ```

### Planet-type tiles

19. **crystal** `(18)` —
    ```
    speckle(px, r, ['#1a4a50','#153c42','#20585e'])
    for i in 0..5:
      x=1+floor(r()*12); y=1+floor(r()*12)
      px(x,y,'#7fe8e0'); px(x+1,y+1,'#aef7f2'); px(x,y+1,'#5ec8c0')
      if r()<0.5: px(x+1,y,'#ffffff')
    ```
    → teal shards on dark base.

20. **mush_stem** `(19)` —
    ```
    for x in 0..16:
      band = ['#e8dcc8','#dccfb8','#f0e6d4'][x % 3]
      for y in 0..16: px(x, y, r()<0.9 ? band : '#c4b8a2')
    for i in 0..16: px(0,i,'#b8ab94'); px(15,i,'#b8ab94')
    ```

21. **mush_cap** `(20)` —
    ```
    speckle(px, r, ['#a04fc8','#9445ba','#ad5cd4','#8a3dad'])
    for i in 0..5:
      x=1+floor(r()*12); y=1+floor(r()*12)
      px(x,y,'#f0e0f8'); px(x+1,y,'#f0e0f8'); px(x,y+1,'#f0e0f8'); px(x+1,y+1,'#e0c8ec')
    ```

22. **ash** `(21)` —
    ```
    speckle(px, r, ['#5c5a56','#524f4c','#66625e','#48453f'])
    for i in 0..3: x=floor(r()*15); y=floor(r()*15); px(x, y, r()<0.5 ? '#8a4a2a' : '#3a3a3a')
    ```

23. **amber** `(22)` —
    ```
    speckle(px, r, ['#e0a63a','#d49830','#ecb448','#c88a28'])
    for i in 0..4:
      x=1+floor(r()*13); y=1+floor(r()*13)
      px(x,y,'#8a5a14'); if r()<0.5: px(x+1,y,'#6e4610')      // inclusion
      px(x-1,y-1,'#f8d878')                                   // highlight
    ```

24. **rust** `(23)` —
    ```
    speckle(px, r, ['#9a5a38','#8a4e30','#a86a42','#7c452a'])
    for i in 0..5:
      x=floor(r()*15); y=floor(r()*15)
      px(x, y, r()<0.5 ? '#c8875a' : '#5e3520')
      if r()<0.3: px(x+1, y, '#d8d8dc')                       // metal glint
    ```

25. **salt** `(24)` —
    ```
    speckle(px, r, ['#f0f2f4','#e6e9ec','#f8fafc','#dde2e6'])
    for i in 0..4: x=1+floor(r()*13); y=1+floor(r()*13); px(x,y,'#c2c9ce'); px(x+1,y,'#c2c9ce'); px(x+1,y+1,'#c2c9ce')
    ```

26. **obsidian** `(25)` —
    ```
    speckle(px, r, ['#1c1a26','#16141f','#24202e','#120f1a'])
    for i in 0..3:
      x=1+floor(r()*12); y=1+floor(r()*12)
      px(x,y,'#6a5a9a'); px(x+1,y+1,'#48406e'); if r()<0.4: px(x+2,y+2,'#8a7ab8')
    ```

27. **redmoss_top** `(26)` — `speckle(px, r, ['#b04a38','#a04230','#c05642','#943a2a','#c86a50'])`.

28. **redmoss_side** `(27)` —
    ```
    speckle(px, r, ['#8a5f3c','#7d5535','#95683f'])
    for x in 0..16: h = 3 + floor(r()*2.2); for y in 0..h: px(x, y, ['#b04a38','#c05642'][floor(r()*2)])
    ```

29. **hive** `(28)` —
    ```
    speckle(px, r, ['#d8862a','#c87822','#e69634'])
    for cy in 0..2 for cx in 0..2:
      ox = cx*8 + (cy%2)*4 ; oy = cy*8
      for a in 0..12:
        x = (ox + 3 + round(cos(a/12*2π)*2.6)) & 15
        y = (oy + 3 + round(sin(a/12*2π)*2.6)) & 15
        px(x, y, '#8a5210')
      px((ox+3)&15, (oy+3)&15, '#5e3808')
    ```
    → 4 offset honeycomb cells (each a 12-point ring of radius 2.6 px, hollow dark centers).

30. **murk_top** `(29)` —
    ```
    speckle(px, r, ['#1e5a4c','#1a4f42','#246656','#16453a'])
    for i in 0..4: px(floor(r()*15), floor(r()*15), '#4ee8b8')     // glow dots
    ```

31. **murk_side** `(30)` —
    ```
    speckle(px, r, ['#4a4238','#3f382f','#554c40'])
    for x in 0..16: h = 3 + floor(r()*2); for y in 0..h: px(x, y, ['#1e5a4c','#246656'][floor(r()*2)])
    ```

32. **glow_shroom** `(31)` — hand-drawn cross-face sprite (mostly transparent):
    ```
    px(7,15,'#3a5248'); px(8,14,'#2e453c'); px(7,13,'#3a5248'); px(8,12,'#2e453c')   // stem
    c='#4ee8b8'; h='#b8ffe8'; d='#2aa882'
    px(6,9,c); px(7,9,c); px(8,9,c); px(9,9,c)
    px(5,10,d); px(10,10,d)
    px(6,8,h); px(7,7,h); px(8,8,c); px(9,8,d)
    px(7,10,'#e8fff6'); px(8,10,'#e8fff6')
    ```

### Ores (generic painter)

`orePainter(color, hi, glow, outline)` returns a painter:

```
speckle(px, r, ['#8c8c8c','#828282','#969696','#7a7a7a'])     // stone base

clusters = 3 + floor(r()*2)                                  // 3 or 4 clusters
for c in 0..clusters:
  x = 2 + floor(r()*9);  y = 2 + floor(r()*9)                // start (2..10)
  steps = 4 + floor(r()*3)                                   // 4..6 walk steps
  cells = [];  seen = {}
  for s in 0..steps:
    key = (x,y); if not seen[key]: seen[key]=1; cells.push([x,y])     // dedupe
    x += floor(r()*3) - 1;  y += floor(r()*3) - 1            // random walk, dx,dy ∈ {-1,0,1}
    clamp x,y to [1,14]
  // outline first (so the body covers the interior side, leaving a 1px rim)
  for each cell: for each 4-neighbor (dx,dy): if in-bounds: px(nx, ny, outline)
  // body
  for each cell: px(cx, cy, color)
  // highlight at the cluster's topmost cell (min y; ties keep first)
  top = cell with smallest y
  px(top.x, max(0, top.y - 1), hi)
  if glow && r()<0.7: px(min(15, top.x + 1), top.y, glow)
```

Instances:

33. **coal_ore** `(32)` — `orePainter('#2b2b2b', '#5a5a5a', null, '#1a1a1a')`.
34. **iron_ore** `(33)` — `orePainter('#d8af93', '#f0d2b8', null, '#a87a5e')`.
35. **copper_ore** `(34)` — `orePainter('#d17f4a', '#f0a877', null, '#9a5a2e')`.
36. **titanium_ore** `(35)` — `orePainter('#e6eef4', '#ffffff', null, '#7a8a94')`.
37. **uranium_ore** `(36)` — `orePainter('#69d436', '#a2f078', '#c6ff9e', '#3a8a18')` (the only *glowing* ore: extra `#c6ff9e` glow dot).
38. **gold_ore** `(37)` — `orePainter('#f5cd3a', '#ffe98a', null, '#b8921a')`.

### Plants (cross-face sprites — mostly transparent)

39. **sodium_plant** `(38)` —
    ```
    px(7,15,'#3f7d2c'); px(8,15,'#357024'); px(7,14,'#3f7d2c'); px(8,13,'#3f7d2c'); px(7,12,'#488a33')
    c='#ffd23e'; h='#fff2ae'; d='#d9a80f'
    px(7,8,c); px(8,8,c); px(7,9,c); px(8,9,h)
    px(6,6,c); px(10,7,d); px(7,5,h); px(9,10,d); px(5,9,c); px(9,5,c)
    ```

40. **oxygen_plant** `(39)` —
    ```
    px(8,15,'#3f7d2c'); px(8,14,'#357024'); px(7,13,'#3f7d2c'); px(8,12,'#488a33')
    c='#ff5a4e'; h='#ffb0a8'; d='#c22e24'
    px(7,8,c); px(8,8,c); px(7,9,c); px(8,9,h); px(6,7,d); px(9,7,c); px(6,10,c); px(9,10,d); px(7,6,h); px(8,11,c)
    ```

41. **carbon_fern** `(40)` —
    ```
    for i in 0..12: x=3+floor(r()*10); y=4+floor(r()*11); px(x, y, ['#2e6420','#3f7d2c','#244f19'][floor(r()*3)])
    px(7,15,'#244f19'); px(8,14,'#2e6420'); px(7,13,'#244f19'); px(8,12,'#2e6420')
    ```

### Functional / machine / buildable blocks

42. **glass** `(41)` —
    ```
    for i in 0..16: px(i,0,'#cfeef5'); px(i,15,'#cfeef5'); px(0,i,'#cfeef5'); px(15,i,'#cfeef5')   // frame
    px(3,3,'#ffffffcc'); px(4,4,'#ffffff99'); px(5,5,'#ffffff66')                                   // diagonal shine (alpha)
    ```
    Interior is transparent; the three shine pixels are white at alpha 0xCC/0x99/0x66.

43. **lamp_on** `(42)` —
    ```
    speckle(px, r, ['#ffe9a8','#fff3c8','#ffdf8e'])
    for i in 0..16: px(i,0,'#8a6b2d'); px(i,15,'#8a6b2d'); px(0,i,'#8a6b2d'); px(15,i,'#8a6b2d')
    ```

44. **metal** `(43)` — the generic machine panel:
    ```
    speckle(px, r, ['#9aa7b0','#909da6','#a4b1ba','#8a97a0'])
    for i in 0..16: px(i,0,'#b8c5ce'); px(0,i,'#b8c5ce')      // top+left light bevel
    for i in 0..16: px(i,15,'#6a7780'); px(15,i,'#6a7780')    // bottom+right dark bevel
    px(2,2,'#5f6b73'); px(13,2,'#5f6b73'); px(2,13,'#5f6b73'); px(13,13,'#5f6b73')  // corner screws
    ```

45. **metal_dark** `(44)` —
    ```
    speckle(px, r, ['#4e5a63','#46525b','#57636c'])
    for i in 0..16: px(i,0,'#68747d'); px(0,i,'#68747d'); px(i,15,'#333d44'); px(15,i,'#333d44')
    ```

46. **vent** `(45)` —
    ```
    speckle(px, r, ['#4e5a63','#46525b'])
    for y in (2; y<14; y+=3): for x in 2..14: px(x,y,'#222a30'); px(x,y+1,'#68747d')
    ```

47. **furnace_front** `(46)` —
    ```
    speckle(px, r, ['#8c8c8c','#828282','#969696'])
    for y in 8..14: for x in 4..12: px(x, y, '#1d1d1d')       // dark opening
    for x in 3..13: px(x,7,'#5a5a5a'); px(x,14,'#5a5a5a')      // frame
    ```

48. **furnace_on** `(47)` — same base, but the opening is filled with flame:
    ```
    speckle(px, r, ['#8c8c8c','#828282','#969696'])
    for y in 8..14: for x in 4..12: px(x, y, ['#ff8c1a','#ffb31a','#ff6600','#ffd21a'][floor(r()*4)])
    for x in 3..13: px(x,7,'#5a5a5a'); px(x,14,'#5a5a5a')
    ```
    (`furnace_front`/`furnace_on` is a **two-state toggle**, not a frame animation.)

49. **belt** `(48)` — conveyor belt with chevron arrows:
    ```
    speckle(px, r, ['#3a4148','#333a40','#424a52'])
    for x in 0..16: px(x,0,'#586269'); px(x,15,'#586269')     // rails
    for oy in [2, 10]:
      px(3,oy,'#ffcf4d'); px(4,oy+1,'#ffcf4d'); px(5,oy+2,'#ffcf4d'); px(4,oy+3,'#ffcf4d'); px(3,oy+4,'#ffcf4d')
      px(9,oy,'#e6b23a'); px(10,oy+1,'#e6b23a'); px(11,oy+2,'#e6b23a'); px(10,oy+3,'#e6b23a'); px(9,oy+4,'#e6b23a')
    ```
    (Scroll animation is done by UV offset elsewhere — see Section 9.)

50. **belt_turn** `(49)` — corner conveyor (entry −z / bottom, exit +x / right):
    ```
    speckle(px, r, ['#3a4148','#333a40','#424a52'])
    for x in 0..16: px(x,0,'#586269')                          // bottom rail
    for y in 0..16: px(0,y,'#586269')                          // left rail
    for a in 0..26:
      t = a/25 * π/2
      x  = round(15 - cos(t)*12); y  = round(15 - sin(t)*12); if in-bounds: px(x,y,'#ffcf4d')
      x2 = round(15 - cos(t)*6);  y2 = round(15 - sin(t)*6);  if in-bounds: px(x2,y2,'#e6b23a')
    px(13,12,'#ffcf4d'); px(12,13,'#ffcf4d')
    ```

51. **wind_pole** `(50)` —
    ```
    speckle(px, r, ['#c8d2d8','#bcc6cc','#d2dce2'])
    for i in 0..16: px(0,i,'#98a2a8'); px(15,i,'#98a2a8')
    px(7,3,'#8a97a0'); px(8,3,'#8a97a0'); px(7,10,'#8a97a0'); px(8,10,'#8a97a0')
    ```

52. **miner_top** `(51)` —
    ```
    speckle(px, r, ['#9aa7b0','#909da6','#a4b1ba'])
    for y in 4..12: for x in 4..12: px(x,y,'#333d44')          // dark plate
    for i in 5..11: px(i,i,'#ffcf4d'); px(16-i,i,'#ffcf4d')    // X chevrons
    for i in 0..16: px(i,0,'#b8c5ce'); px(0,i,'#b8c5ce'); px(i,15,'#6a7780'); px(15,i,'#6a7780')
    ```

53. **assembler_top** `(52)` —
    ```
    speckle(px, r, ['#9aa7b0','#909da6','#a4b1ba'])
    for y in 3..13: for x in 3..13: px(x,y,'#1a2a38')
    px(7,7,'#35e0e8'); px(8,7,'#35e0e8'); px(7,8,'#35e0e8'); px(8,8,'#7ff5fa')
    for i in 0..16: px(i,0,'#b8c5ce'); px(0,i,'#b8c5ce'); px(i,15,'#6a7780'); px(15,i,'#6a7780')
    ```

54. **solar_top** `(53)` —
    ```
    for y in 0..16 for x in 0..16:
      px(x, y, (x%5==0 || y%8==7) ? '#8a97a0' : ['#16294e','#1a3160','#122342'][floor(r()*3)])
    px(3,2,'#4a6dc0'); px(8,4,'#4a6dc0'); px(12,9,'#4a6dc0')
    ```
    → grid of panel lines on deep-blue cells.

55. **chest_side** `(54)` —
    ```
    speckle(px, r, ['#a8824f','#9d7948','#b28a55'])
    for i in 0..16: px(i,0,'#7a5c35'); px(i,15,'#7a5c35'); px(0,i,'#7a5c35'); px(15,i,'#7a5c35')
    for x in 0..16: px(x,6,'#63482a')                          // strap
    px(7,6,'#d8d8d8'); px(8,6,'#d8d8d8'); px(7,7,'#b8b8b8'); px(8,7,'#b8b8b8')   // latch
    ```

56. **refinery_side** `(55)` —
    ```
    speckle(px, r, ['#4e5a63','#46525b','#57636c'])
    for y in 3..13: px(4,y,'#ff8c1a'); px(5,y,'#c9641a'); px(10,y,'#35e0e8'); px(11,y,'#1a8a90')
    for i in 0..16: px(i,0,'#68747d'); px(i,15,'#333d44')
    ```

57. **reactor_side** `(56)` —
    ```
    speckle(px, r, ['#4e5a63','#46525b','#57636c'])
    for y in 4..12: for x in 6..10: px(x,y,['#69d436','#a2f078','#4caf1e'][floor(r()*3)])
    for i in 0..16: px(i,0,'#68747d'); px(0,i,'#68747d'); px(i,15,'#333d44'); px(15,i,'#333d44')
    ```

58. **launchpad_top** `(57)` —
    ```
    speckle(px, r, ['#4e5a63','#46525b'])
    for i in 0..16: if i%4<2: px(i,0,'#ffcf4d'); px(i,15,'#ffcf4d'); px(0,i,'#ffcf4d'); px(15,i,'#ffcf4d')   // dashed border
    for a in 0..40:
      x = 8 + round(cos(a/40*2π)*5); y = 8 + round(sin(a/40*2π)*5); if in-bounds: px(x,y,'#ffcf4d')          // circle r=5
    px(7,8,'#ffcf4d'); px(8,8,'#ffcf4d'); px(8,7,'#ffcf4d'); px(7,7,'#ffcf4d')
    ```

59. **storage_top** `(58)` —
    ```
    speckle(px, r, ['#a8824f','#9d7948'])
    for i in 0..16: px(i,0,'#7a5c35'); px(i,15,'#7a5c35'); px(0,i,'#7a5c35'); px(15,i,'#7a5c35')
    ```

60. **medbay_top** `(59)` —
    ```
    speckle(px, r, ['#4e5a63','#46525b','#57636c'])
    for y in 4..11: px(7,y,'#7dff8a'); px(8,y,'#7dff8a')      // green cross (vertical)
    for x in 4..11: px(x,7,'#7dff8a'); px(x,8,'#7dff8a')      // green cross (horizontal)
    for i in 0..16: px(i,0,'#68747d'); px(0,i,'#68747d'); px(i,15,'#333d44'); px(15,i,'#333d44')
    ```

61. **slab** `(60)` — stone half-slab:
    ```
    speckle(px, r, ['#8c8c8c','#828282','#969696','#7a7a7a'])
    for i in 0..16: px(i,0,'#a8a8a8'); px(i,1,'#9c9c9c')      // smooth top cut
    for i in 0..16: px(i,15,'#5a5a5a')                        // bottom shadow
    for x in (2; x<14; x+=4): px(x,8,'#9c9c9c'); px(x+1,9,'#9c9c9c')   // x = 2,6,10
    ```

62. **concrete** `(61)` —
    ```
    speckle(px, r, ['#9aa3ab','#8f989f','#a5aeb6','#848d94'])
    for i in 0..16: px(i,0,'#b8c0c7'); px(0,i,'#a8b0b8')
    px(3,4,'#7a828a'); px(4,5,'#7a828a'); px(11,9,'#7a828a'); px(12,10,'#7a828a')
    ```

> **Total: 62 tiles** (indices 0–61). The remaining 194 slots of the 256-slot atlas are unused.
> If you want byte-identical output, register the tiles in the exact order above and keep the exact
> RNG call order inside each painter.

---

## 4. Item icons (`Icons`) — 32×32 canvases, lazy-cached

- Every icon is a **32×32** canvas. `P(ctx) = (x, y, col, w=1, h=1)` draws a filled rect at `(x,y,w,h)`.
- `newC()` creates a fresh 32×32 canvas; results are cached by `itemId` and cloned on demand via
  `Icons.img(itemId)`.

### 4.1 Isometric block icon (`blockIcon(topName, sideName, side2Name)`)

`ctx.imageSmoothingEnabled = false`. Three faces of a block are drawn with 2D transforms (darkening via
`globalCompositeOperation='source-atop'`):

```
// Top face (diamond): scale the 16×16 tile into a 15×15 skewed quad
save(); translate(16, 1); transform(1, 0.5, -1, 0.5, 0, 0);
drawImage(top, 0,0,16,16, 0,0,15,15); restore();

// Left face (skewed down-right)
save(); translate(1, 8.5); transform(1, 0.5, 0, 1, 0, 0);
drawImage(side, 0,0,16,16, 0,0,15,15.5);
globalCompositeOperation='source-atop'; fillStyle='rgba(0,0,0,0.25)'; fillRect(0,0,16,24); restore();

// Right face (skewed up-right)
save(); translate(16, 16); transform(1, -0.5, 0, 1, 0, 0);
drawImage(side2, 0,0,16,16, 0,0,15,15.5);
globalCompositeOperation='source-atop'; fillStyle='rgba(0,0,0,0.45)'; fillRect(0,0,16,24); restore();
```

- Top face: **unshaded**. Left face: **25 % black** overlay. Right face: **45 % black** overlay.
- The `drawImage` scales the 16×16 source into a ~15×15 / 15×15.5 destination with nearest sampling.
- In Rust you must implement the affine transform (2×2 matrix `[1,0.5; -1,0.5]`, `[1,0.5; 0,1]`,
  `[1,-0.5; 0,1]`) with nearest-neighbor resampling, then multiply the RGB of covered pixels by 0.75
  (left) and 0.55 (right).

### 4.2 Which icon each item gets (`Icons.get(itemId)`)

```
def = ITEMS[itemId]
if !def:                    -> blank 32×32
else if def.iconBlock:
  b = BLOCKS[def.iconBlock]
  if b.cross:               -> flatIcon(b.tiles.side || b.tiles.all)     // plants: flat sprite
  else:                     -> blockIcon(b.tiles.top||b.tiles.all,
                                          b.tiles.side||b.tiles.all,
                                          b.tiles.front||b.tiles.side||b.tiles.all)
else if def.iconFn in painters: -> painters[def.iconFn]()
else:                       -> crystalIcon('#888888', '#cccccc')        // generic fallback
```

### 4.3 `flatIcon(tileName)`

```
drawImage(tileCanvas(tileName), 0,0,16,16, 0,0,32,32)   // integer 2× nearest scale
```

### 4.4 Shape helpers

- **ingotIcon(c1, c2)** — `dark = shade(c1,0.6)`, `hi = c2`:
  ```
  px(6,16,dark,20,8); px(4,14,c1,20,8); px(4,12,hi,20,3)
  px(6,24,shade(c1,0.45),20,1)
  px(5,13,'#ffffff88',8,1)          // white streak at alpha 0x88
  (strokeStyle=shade(c1,0.4), lineWidth=1 set but no stroke path drawn — a no-op)
  ```
- **crystalIcon(c1, c2)** — `d = shade(c1,0.55)`:
  ```
  px(14,4,c2,4,4); px(12,8,c1,8,10); px(10,12,d,4,8); px(18,10,c1,6,12)
  px(8,18,c1,6,8); px(20,6,c2,2,4); px(15,9,'#ffffffaa',2,5); px(6,26,d,20,2)
  ```
- **chunkIcon(c1)** — `d = shade(c1,0.6)`, `h = shade(c1,1.35)`:
  ```
  px(8,10,c1,10,9); px(16,14,d,8,8); px(10,18,d,8,6); px(12,8,h,4,3)
  px(20,12,h,3,2); px(7,14,d,3,5)
  ```

### 4.5 Named painters (`painters = { ... }`)

- **gear()** — `g='#aab6bf'`, `d='#77848d'`, `h='#d5dde2'`:
  ```
  for a in 0..8: x=16+round(cos(a/8*2π)*11)-2; y=16+round(sin(a/8*2π)*11)-2; px(x,y,d,5,5)   // 8 teeth
  fill arc(16,16, r=9) g ; fill arc(14,14, r=4) h ; fill arc(16,16, r=4) '#2c353b'
  ```
- **circuit()** — green PCB:
  ```
  px(5,7,'#1d7a3c',22,18); px(5,7,'#25914a',22,3)
  px(9,12,'#ffd24d',5,5); px(19,16,'#2c353b',6,4)
  px(7,20,'#d17f4a',16,1); px(7,10,'#d17f4a',1,11); px(14,14,'#d17f4a',8,1); px(24,9,'#d17f4a',1,8)
  px(11,22,'#c0c0c0',2,3); px(17,22,'#c0c0c0',2,3)
  ```
- **data()** — data chip:
  ```
  px(6,6,'#122c48',20,20); px(6,6,'#1a3d63',20,4)
  px(10,13,'#35e0e8',12,2); px(10,17,'#35e0e8',8,2); px(10,21,'#2596a0',10,1)
  px(24,12,'#7dff8a',2,2)
  for i in 0..4: px(8+i*5,3,'#8a97a0',2,3); px(8+i*5,26,'#8a97a0',2,3)
  ```
- **fuel()** — fuel canister:
  ```
  px(10,6,'#8a97a0',12,4); px(8,10,'#c0392b',16,16); px(8,10,'#e74c3c',16,5)
  px(12,15,'#f8d347',8,7); px(14,17,'#c0392b',4,3)
  px(8,26,'#7f2418',16,2); px(13,3,'#5f6b73',6,3)
  ```
- **tritium()** = `crystalIcon('#4da6ff', '#b3dbff')`.
- **oxygen()** — four overlapping red circles (filled arcs):
  ```
  arc(13,14,8) '#c2392b'; arc(20,20,6) '#e74c3c'; arc(10,11,3) '#ffb3ab'; arc(19,18,2) '#ff8a80'
  ```
- **carbon()** = `crystalIcon('#3a3a3a', '#6e6e6e')`.
- **sodium()** = `crystalIcon('#ffd23e', '#fff2ae')`.
- **uranium()** = `crystalIcon('#69d436', '#c6ff9e')`.
- **coal()** = `chunkIcon('#2f2f2f')`.
- **iron_ore()** = `chunkIcon('#d8af93')`.
- **copper_ore()** = `chunkIcon('#d17f4a')`.
- **titanium_ore()** = `chunkIcon('#cdd6dd')`.
- **gold_ore()** = `chunkIcon('#f5cd3a')`.
- **iron()** = `ingotIcon('#b8c4cc', '#e2eaef')`.
- **copper()** = `ingotIcon('#d17f4a', '#f0a877')`.
- **titanium()** = `ingotIcon('#dfe8ee', '#ffffff')`.
- **gold()** = `ingotIcon('#f5cd3a', '#ffe98a')`.
- **glass_item()** = `flatIcon('glass')`.
- **stone_item()** = `chunkIcon('#8c8c8c')`.
- **wire()** — coil:
  ```
  stroke '#d17f4a', lineWidth 3, arc(16,16,9, 0.5..5.5 rad)
  stroke '#f0a877', lineWidth 1, arc(16,15,9, 0.7..5.3 rad)
  ```
- **plate()** — metal plate:
  ```
  px(6,8,'#8a97a0',20,16); px(6,8,'#aab6bf',20,4); px(6,22,'#5f6b73',20,2)
  px(9,11,'#4a545b',2,2); px(21,11,'#4a545b',2,2); px(9,19,'#4a545b',2,2); px(21,19,'#4a545b',2,2)
  ```
- **warp()** — radial gradient `(16,16, r2 → 16,16, r13)`:
  ```
  0.0 '#e0d0ff'; 0.5 '#b48cff'; 1.0 '#3a1d66' ; fill arc(16,16,12)
  ellipse stroke '#e0d0ffaa' (α=0xAA), lineWidth 2, ellipse(16,16, rx13, ry5, rot -0.6)
  ```
- **antimatter()** — radial gradient `(16,16, r1 → 16,16, r12)`:
  ```
  0.0 '#000000'; 0.55 '#1a0a2e'; 0.8 '#e838a8'; 1.0 '#40103080' (α=0x80) ; fill arc(16,16,12)
  ellipse stroke '#ff66ccdd' (α=0xDD), lineWidth 2, ellipse(16,16, rx13.5, ry4.5, rot 0.8)
  ellipse stroke '#ffffff88' (α=0x88), lineWidth 1, ellipse(16,16, rx12.5, ry3.5, rot 0.8)
  fillRect(15,15,2,2) '#ffffff'    // white singularity center
  ```

### 4.6 Extra icon: mining laser (`js/ui.js` `laserIcon()`, 32×32)

```
px(6,14,'#4e5a63',16,6)    // body
px(8,12,'#68747d',12,2)    // top cover
px(20,15,'#333d44',8,4)    // barrel
px(27,14,'#c9641a',2,6)    // muzzle ring
px(9,20,'#333d44',3,6)     // grip
px(10,15,'#35e0e8',5,3)    // energy screen
px(5,15,'#c9641a',2,4)     // tail accent
```

### 4.7 Hotbar rendering

The hotbar (`ui.js` `buildHotbar`) is 9 DOM slots + a fixed laser slot. Each slot displays
`Icons.img(itemId)` (a 32×32 canvas) scaled by CSS `.hslot canvas { width:82%; height:82%;
image-rendering:pixelated }`. Icons are therefore **nearest-upscaled by CSS** (no extra draw pass).

---

## 5. Space-station tiles (`StationTex`) — 64×64, metal/panel/hatch/stripe set

`TS = 64`; one shared `rnd = mulberry32(20240107)` stream is used **across all tiles in order**, so the
station tiles are order-dependent too. Each tile is a 64×64 canvas. The resulting texture uses the same
`Nearest`/`NearestMipmapNearest`/mipmaps settings (Section 1.3) with `RepeatWrapping` and
`repeat.set(repX, repY)` (defaults 1,1).

Shared subroutines:

- **metalBase(ctx, pal)** — weighted speckle, thresholds fixed:
  ```
  for each pixel: r = rnd()
    color = pal[0] if r < 0.45   // 45 %
        else pal[1] if r < 0.75   // 30 %
        else pal[2] if r < 0.92   // 17 %
        else pal[3]               //  8 %
  ```
- **panelLines(ctx, step, col)** — 1px horizontal + vertical lines every `step` px.
- **rivets(ctx, step, col)** — at `(rx,ry)` on a `step` grid starting at `step/2`: `fillRect(rx-1,ry-1,2,2)`
  and `fillRect(rx+1,ry+1,1,1)` (2×2 rivet + 1×1 offset dot).
- **edge(ctx, top, bot)** — 2px top + 2px left = `top` color; 2px bottom + 2px right = `bot` color.
- **hatch(ctx, cx, cy, r, col, dark)** — filled circle `r` in `col`, inner circle `r*0.6` in `dark`, plus
  a horizontal bar `(cx-r, cy-1, r*2, 2)` and vertical bar `(cx-1, cy-r, 2, r*2)` in `col`.
- **stripeBand(ctx, y0, h)** — clipped diagonal hazard band. For `i = -TS; i < 2*TS; i += 12`, fill
  parallelogram `(i,y0+h)→(i+8,y0)→(i+12,y0)→(i+4,y0+h)` with `i%24==0 ? '#e8b428' : '#2a2e34'`; then 1px
  top/bottom border `'#8a939c'`.

Tiles (`make(name, painter)`):

- **panel_a** — `metalBase(['#7c8894','#8a97a0','#6e7a86','#96a4ae'])`; `panelLines(16,'#4e5a63')`;
  `rivets(16,'#c2ccd4')`; `edge('#aab6bf','#3c454d')`; corner plates `fillRect(4,4,10,10)` and
  `fillRect(50,50,10,10)` in `'#5f6b73'`.
- **panel_b** — `metalBase(['#6a7682','#78848f','#5e6a75','#84909b'])`; `panelLines(32,'#46525b')`;
  X braces: dark stroke `'#3a454d'` lineWidth 3 (corner-to-corner both diagonals) + light stroke
  `'#c2ccd4'` lineWidth 1 (offset diagonals); `hatch(32,32,8,'#5a666f','#2e363d')`; `rivets(32,'#b6c0c8')`;
  `edge('#aab6bf','#3c454d')`.
- **panel_c** — `metalBase(['#414c55','#4a555e','#39434b','#525d66'])`; `panelLines(16,'#2c353c')`;
  `stripeBand(50, 14)`; `hatch(22,24,9,'#3a454d','#222a30')`; 1px top edge `'#68747d'` + 1px right edge
  `'#68747d'`; `rivets(32,'#7c8894')`.
- **deck_plate** — `metalBase(['#39434b','#414c55','#333d44','#48535c'])`; anti-slip bars: for `y` step 8,
  `fillRect(0,y,64,2)` in `y%16==0 ? '#5a666f' : '#46525b'`; thin shadow `fillRect(0,y,64,1)` `'#2c353c'`
  for `y=4;y<64;y+=8`; `stripeBand(0,6)` and `stripeBand(58,6)`.
- **cargo_door** — `metalBase(['#55616b','#5e6a75','#4a555e','#6a7682'])`; `panelLines(16,'#3c454d')`;
  center seam `fillRect(30,0,4,64)` `'#2c353c'`; two handles `fillRect(10,29,8,6)` and
  `fillRect(46,29,8,6)` `'#222a30'` with top highlight `'#8a939c'` `fillRect(10,29,8,2)` /
  `fillRect(46,29,8,2)`; `stripeBand(0,8)` and `stripeBand(56,8)`; `rivets(32,'#9aa5ae')`.
- **window_band** — `metalBase(['#4a555e','#525d66','#414c55','#5a666f'])`; glass
  `fillRect(6,8,52,48)` `'#0b1418'`; cyan glow `fillRect(8,22,48,10)` `'#35e0e8'` with highlight
  `fillRect(8,22,48,3)` `'#7ff5fa'`; lower strip `fillRect(8,40,48,6)` `'#0f4a52'`; frame bars
  `'#8a939c'`: for `i=8;i<64;i+=12` `fillRect(i,6,2,52)` and `fillRect(i,56,2,2)`; `edge('#8a939c','#333d44')`.
- **vent_grille** — `metalBase(['#46525b','#4e5a63','#3e4952','#56626b'])`; louvers `'#222a30'`:
  for `y=6;y<58;y+=8` `fillRect(6,y,52,3)` then `fillRect(6,y+3,52,1)` in `y%16==6 ? '#222a30' : '#2e363d'`;
  `edge('#68747d','#2c353c')`.
- **logo_stripe** — base `fillRect(0,0,64,64)` `'#a04e14'`; horizontal bands for `y` step 12:
  `fillRect(0,y,64,4)` `'#c9641a'` and `fillRect(0,y+6,64,4)` `'#7a3a0e'`; chevrons `'#ffb347'`: for
  `i in 0..3` at `cy = 14 + i*20`, draw triangle `(12,cy)→(26,cy+6)→(12,cy+12)` and mirrored
  `(52,cy)→(38,cy+6)→(52,cy+12)`.

---

## 6. Planet / sun / galaxy / cloud / reticle textures (cross-file)

### 6.1 Noise generator (`js/world.js`, Appendix A) and `mulberry32` (Section 2.1) are prerequisites.

### 6.2 Planet surface texture — `space.js planetTexture(biomeKey, seed)`

- **256×128** canvas, longitude-seamless, per-biome elevation field.
- `n = makeNoise(seed)`; `rnd = mulberry32((seed ^ 0xB10B) >>> 0)`.
- Palette table `PLANET_PAL[biomeKey]` (5 colors each; index 0 = sea, 1 = shore, 2 = lowland,
  3 = highland, 4 = feature accent) — **exact values**:

  | biome | sea | shore | lowland | highland | accent |
  |---|---|---|---|---|---|
  | lush | `2b62c8` | `e8dca0` | `6fbf44` | `3e8a2e` | `9fe06a` |
  | desert | `6db8c8` | `f0e0a0` | `e0c47a` | `c89a52` | `e8e0b0` |
  | frozen | `9fd4e8` | `e8f2f8` | `dfeef8` | `ffffff` | `cfe8f8` |
  | volcanic | `ff6a1a` | `5a4038` | `3a3a42` | `2a2a30` | `ff8a2a` |
  | alien | `7a4ad8` | `b06fe0` | `9a5fd0` | `6a3ab8` | `e08aff` |
  | ocean | `2b62c8` | `4a82d8` | `3e8ed6` | `7cc44f` | `6fbf44` |
  | crystal | `8fd8e8` | `e8f6fa` | `cfeef6` | `7fe8e0` | `aef7f2` |
  | fungal | `6a4a8a` | `c06fd8` | `9a4ab8` | `e8a0f0` | `7a3a9a` |
  | ashen | `9a7a5a` | `8a8a8a` | `6e6a66` | `52504c` | `a89888` |
  | amber | `d8b048` | `e8c060` | `e0a63a` | `b08028` | `f0d078` |
  | ferrous | `8a5a3a` | `a86a4a` | `7c4a30` | `5e3824` | `c8875a` |
  | murk | `2f7a5a` | `2e8a72` | `1e5a4c` | `16453a` | `4ee8b8` |
  | salt | `cfe8f0` | `f0f2f4` | `e8ecf0` | `dde2e6` | `ffffff` |
  | obsidian | `4a3a6a` | `2a2a35` | `1c1a26` | `120f1a` | `6a5a9a` |
  | redmoss | `b06050` | `c25a48` | `943a2a` | `6e2a1e` | `d88068` |
  | hive | `d89830` | `d8862a` | `b06a18` | `8a5210` | `e8a840` |

- Per-pixel procedure:
  ```
  lat  = (0.5 - (py+0.5)/128) * π
  wy   = sin(lat) * 40
  polar= max(0, abs(lat) - (icy ? 0.72 : 1.05)) / (icy ? 0.6 : 0.35)   // icy = frozen|crystal
  lon  = π - (px+0.5)/256 * 2π
  wx   = cos(lon) * 40
  e    = elev(wx, wy)              // per-biome, see below
  if e < 0:        d=min(1,-e*2.2); col = lerp(pal[0], pal[1], d)          // sea→shore
  else if e<0.28:  col = pal[2]                                            // lowland
  else:            col = lerp(pal[2], pal[3], min(1,(e-0.28)*2.2))        // lowland→highland
  // feature accents (after base color):
  murk && e>=0 && rnd()<0.04   -> col = lerp(col, pal[4], 0.85)
  icy  && e>=0.18 && rnd()<0.025 -> col = lerp(col, pal[4], 0.85)
  lush && e>=0 && rnd()<0.02   -> col = lerp(col, pal[4], 0.7)
  volcanic && e<0.05 && rnd()<0.3 -> col = lerp(col, pal[0], 0.9)         // lava veins
  ashen && e>=0 && rnd()<0.015 -> col = lerp(col, pal[4], 0.6)
  salt && e>=0 && rnd()<0.02   -> col = lerp(col, pal[4], 0.8)
  // polar caps (skip for volcanic/ashen):
  if polar>0 && biome not in {volcanic,ashen}:
    p = min(1, polar * (icy?1.4:0.9)) * (icy?0.95:0.55)
    col = lerp(col, 0xffffff, p)
  out RGBA8 = (round(col.r*255), round(col.g*255), round(col.b*255), 255)
  ```
- `elev(wx, wy)` per biome (all use `n.fbm2`):
  - desert/amber: `sin(wx*3.2 + fbm2(wx*0.02,wy*0.02,3)*2.4)` → `d`; return
    `fbm2(wx*0.014,wy*0.014,4)*0.8 + d*0.12`.
  - ocean: `v = fbm2(wx*0.018,wy*0.018,3)*0.5+0.5; m=max(0,(v-0.47)/0.17); return pow(m,1.5)*1.4 - 0.55`.
  - volcanic: `ridge = max(0, 1 - abs(fbm2(wx*0.03+40,wy*0.03,4))*1.7 - 0.18); return ridge*1.1 - 0.35 + fbm2(wx*0.012,wy*0.012,4)*0.3`.
  - frozen/crystal: `fbm2(wx*0.012,wy*0.012,4)*0.75`.
  - alien: `v=fbm2(wx*0.02,wy*0.02,3)*0.5+0.5; return max(0,(v-0.5)/0.5)*1.6 - 0.5`.
  - ashen/salt: `fbm2(wx*0.01,wy*0.01,3)*0.4`.
  - murk: `fbm2(wx*0.011,wy*0.011,3)*0.5`.
  - default: `fbm2(wx*0.012,wy*0.012,4) + fbm2(wx*0.05,wy*0.05,3)*0.3`.
- Colors are written **in sRGB space directly** (no gamma/pow(1/2.2) correction — the code explicitly
  avoids it so space and ground match).
- Texture settings: `magFilter=Nearest`, `minFilter=NearestMipmapNearest`, `generateMipmaps=true`
  (256×128 is power-of-two). Two full copies (`cleanCanvas`, `origCanvas`) are kept for re-painting /
  restoration by the terrain editor.

### 6.3 Sun textures — `space.js sunTextures()`

- **Surface** (W×H via `makeNoise(20770)`): per pixel,
  ```
  g = fbm2(px*0.07, py*0.1, 4)*0.5+0.5          // granulation
  w = fbm2(px*0.018+40, py*0.026, 3)*0.5+0.5     // large convection
  t = 0.4 + g*0.42 + w*0.28
  R=255, G=min(255, 130 + t*125), B=min(255, 20 + t*130), A=255
  ```
- **Corona** (128×128): radial gradient `(64,64,r4 → 64,64,r64)`,
  stops `0:'rgba(255,240,180,0.9)', 0.4:'rgba(255,200,120,0.3)', 1:'rgba(255,180,80,0)'`.

### 6.4 Galaxy sprite — `space.js galaxyCanvas(seed)` (128×128)

```
rnd = mulberry32(seed >>> 0);  hue = rnd()*360
radial core (cx=cy=64, r2 → r=128*0.46): 0 'hsla(hue,80%,88%,0.95)', 0.25 'hsla(hue,70%,62%,0.5)', 1 'hsla(0,0%,0%,0)'
arms = 2 + floor(rnd()*2)              // 2 or 3 spiral arms
for a in 0..arms:
  a0 = a/arms*2π + rnd()
  for i in 0..90:
    t = i/90;  ang = a0 + t*4.2;  r = 4 + t*128*0.42
    color = hsla(hue + t*40, 70%, (70 - t*25)%, 0.5*(1-t))
    fillRect(cx + cos(ang)*r + (rnd()-0.5)*5, cy + sin(ang)*r*0.62 + (rnd()-0.5)*5, 1.6, 1.6)
```

### 6.5 Planet map (M key) — `js/main.js`

The "planet map" is **not** a 2D texture: it is a live 3D WebGL holographic globe (`#mapCanvas` canvas +
markers + point-lights), rendered with a dedicated renderer each frame. There is no pixel-art bitmap to
port. The galaxy map (`ui.js`) is likewise a live 3D scene of sprite billboards. Only the sprite/corona/
reticle **textures** are procedural (below).

### 6.6 Star, reticle, crosshair

- **starTexture(color, spikes)** (`ui.js`, 128×128) — radial gradient `(64,64,r2 → 64,64,r62)`:
  `0 '#ffffff'`, `0.2 color`, `1 'rgba(0,0,0,0)'`. If `spikes`: additive (`lighter`) linear-gradient
  horizontal line `fillRect(6,63,116,2)` with gradient `(6,0→122,0)` stops
  `0 rgba(255,255,255,0) / 0.5 rgba(255,255,255,0.9) / 1 rgba(255,255,255,0)`, plus the same rotated 90°.
- **reticleTexture(color)** (`ui.js`, 128×128) — two opposing round-cap arcs:
  ```
  strokeStyle=color, lineWidth=5, lineCap='round', shadowColor=color, shadowBlur=8
  arc(64,64,50, -0.35 .. 1.25 rad)          // upper arc
  arc(64,64,50, π-0.35 .. π+1.25 rad)       // lower arc (mirrored)
  ```
  Used as the galaxy-map selection marker (sprites scaled 16 and 21).
- **In-game crosshair** — pure CSS (no texture): `index.html` `#crosshair` is a 22×22 DOM box whose
  `::before`/`::after` are 2px `#eafcffcc` lines with `box-shadow 0 0 4px #35e0e8`, plus a
  `1px solid #35e0e888` centered circle (`span`, inset 6px). Reproduce as an overlay, not a bitmap.

---

## 7. Player / humanoid — NOT a skin texture (9 customizable parts)

The humanoid is built in `js/humanoid.js` as an **SVG-profile extrusion**, not a UV-mapped texture. To
port, you extrude each SVG path (48×104 viewBox) to a depth and tint it with a flat color. There are
**no pixel coordinates for a skin atlas** — the "coordinates" are SVG paths.

Scale: `S = 0.0172` world units per SVG unit (full height ≈ 104·S ≈ 1.79). Y is flipped on mesh build
(`scale.set(S, -S, S)`); ground baseline is SVG `y=100` (boots).

### 7.1 Part list (path `d`, extrusion `depth`, `z` offset, joint pivot)

| part | fill color | path (SVG, viewBox 48×104) | depth | z | pivot |
|---|---|---|---|---|---|
| hair | hair | `M15,4 Q24,-2 33,4 L34,10 L14,10 Z` | 13.5 | – | (24,18) |
| face | skin | `M15,6 Q24,2 33,6 L33,15 Q24,19 15,15 Z` | 13 | – | (24,18) |
| neck | skin | `M21,15 L27,15 L27,20 L21,20 Z` | 13 | – | (24,18) |
| hairBack | hair | `M15,6 Q24,2 33,6 L33,14 Q24,18 15,14 Z` | 2.5 | 5.0 | (24,18) |
| eyeL | `#20262e` (fixed) | `M18.6,9.5 L21.4,9.5 L21.4,12.4 L18.6,12.4 Z` | 2.5 | −7.3 | (24,18) |
| eyeR | `#20262e` | `M26.6,9.5 L29.4,9.5 L29.4,12.4 L26.6,12.4 Z` | 2.5 | −7.3 | (24,18) |
| torso | suit | `M13,19 L35,19 Q38,20 38,26 L36,54 L12,54 L10,26 Q10,20 13,19 Z` | 12 | – | (24,56) |
| trim | trim (optional) | `M22,19 L26,19 L26,54 L22,54 Z` | 12.5 | – | (24,56) |
| belt | belt | `M12,54 L36,54 L36,58 L12,58 Z` | 13 | – | (24,56) |
| armL | suit | `M7,21 L13,20 L12,47 L6,46 Z` | 12 | – | (13,20) |
| handL | glove | `M6,46 L12,47 L11.5,53 L5.8,52 Z` | 12 | – | (13,20) |
| armR | suit | `M35,20 L41,21 L42,46 L36,47 Z` | 12 | – | (35,20) |
| handR | glove | `M36,47 L42,46 L42.2,52 L36.5,53 Z` | 12 | – | (35,20) |
| legL | pants | `M13,58 L22.4,58 L21.4,93 L14,93 Z` | 11 | – | (17.5,56) |
| bootL | boots | `M13.4,93 L21.6,93 L22,100 L12.4,100 Z` | 12 | – | (17.5,56) |
| legR | pants | `M25.6,58 L35,58 L34,93 L26.6,93 Z` | 11 | – | (30.5,56) |
| bootR | boots | `M26.4,93 L34.6,93 L35.6,100 L26,100 Z` | 12 | – | (30.5,56) |

Extrusion: `depth` = thickness along Z (world units), `bevelEnabled=false`, `curveSegments=8`; geometry
translated by `(-pivot.x, -pivot.y, z ?? -depth/2)` then scaled `(S, -S, S)`. `basic:true` parts (eyes)
use unlit `MeshBasicMaterial`; all others `MeshLambertMaterial`.

### 7.2 Hair styles (`HAIR_STYLES`) — additional paths appended to the head group

| style | extra path(s) | depth | z |
|---|---|---|---|
| none | (none; also skips `hair`/`hairBack`) | – | – |
| short | (none — base hair only) | – | – |
| long | `M14,8 L34,8 L34,36 Q34,40 30,40 L18,40 Q14,40 14,36 Z` | 2.5 | 5.5 |
| pony | `M19,12 Q30,8 34,17 L37,36 L33,41 L28,33 L26,16 Z` | 2.5 | 5.5 |
| mohawk | `M19,5 L29,5 L27,-4 L24,0 L21,-4 Z` | 13 | – |
| bun | `M19,2 A5,5 0 1,1 29,2 A5,5 0 1,1 19,2 Z` | 13 | – |

### 7.3 Customizable colors & defaults

`build(opt)` defaults: `skin #e8c49a`, `hair #4a3018`, `suit #4a5a6e`, `trim = suit`, `pants #33404c`,
`boots #1e262e`, `glove #2e3640`, `belt #22303a`, eyes fixed `#20262e`. Optional toggles: `trimOn`,
`helmet`, `visor`, `badge`, `jetpack`.

Character-creation palettes (`ui.js`):

- **CC_SKINS** `['#e8c49a','#d8b48a','#c89878','#8d5a3c','#6b4630','#f0d8b8','#b98e6a','#e8d0b0']`
- **CC_HAIRSTYLES** `none/无, short/短发, long/长发, pony/马尾, mohawk/莫霍克, bun/发髻`
- **CC_HAIRS** `['#4a3018','#2e2620','#5a4632','#7a5a8a','#a86a3a','#d8c8a8','#c23a3a','#1e2e4a']`
- **CC_SUITS** `['#4a5a6e','#3fa8c9','#5a3e3e','#6e6a2a','#3e5a6e','#4a4258','#5a6a3a','#7a3a2a']`
- **CC_TRIMS** `['#35e0e8','#ffb347','#ff6a5e','#b58aff','#7dff8a','#ffd94d','#f0f0f0','#35b0ff']`
- **CC_PANTS** `['#33404c','#4a3c2e','#2e3a44','#3a3248','#3e3a2e','#443430']`
- **CC_BOOTS** `['#1e262e','#2e2620','#26221a','#241e2e','#2a221e','#33261a']`
- **CC_VISORS** `['#ffb347','#35e0e8','#ff6a5e','#b58aff','#7dff8a','#f0f0f0']`

### 7.4 Accessories (world-unit boxes, not textures)

- **helmet**: `BoxGeometry(0.5,0.44,0.48)`, suit color, at `(0,(18-10)·S,0)` on head.
- **visor**: `BoxGeometry(0.3,0.15,0.04)`, color = visor with `emissive = visor·0.25`, at
  `(0,(18-10.5)·S,-0.25)`.
- **badge**: `BoxGeometry(0.13,0.13,0.03)`, trim color, at `(0,(56-37)·S,-0.165)` on torso.
- **jetpack**: `BoxGeometry(0.3,0.44,0.15)` (color `#1d3a52` if `true`), at `(0,(56-38)·S,0.19)`, plus two
  `CylinderGeometry(0.045,0.045,0.42,6)` tanks `#8fa8b8` at `(±0.09,(56-38)·S,0.3)`.

### 7.5 Animation (walk/idle) — for reference only

`animate(g, dt, moving, speed)`: a blend factor `k` eases to 1 when moving (`k += (target-k)·min(1,dt·7)`);
time `t += dt·(2.2 + speed·2.6)·(0.5+k)`; `s=sin(t)`; `amp=min(0.62, 0.3+speed·0.22)`. Then
`legL.rot.x = s·amp·k`, `legR.rot.x = -s·amp·k`, `armL.rot.x = -s·amp·0.85·k`, `armR.rot.x = s·amp·0.85·k`,
`torso.rot.x = 0.05·k + sin(t·2)·0.02·k`, `torso.pos.y = baseY + |s|·0.03·k`, `head.rot.x = -sin(t·2)·0.035·k`,
idle breath `torso.scale.y = 1 + sin(t·0.55)·0.016·(1-k)`.

---

## 8. Creatures (`js/creatures.js`) — NOT textures

All creatures are **box geometries or external GLB models** with flat `MeshLambertMaterial` colors; there
is no procedural creature texture.

- **Sentinel (ruin guard)**: body `0x2c333f` (box 0.5×0.26×0.5), dark `0x1a2129` (two crossed rotor arms
  `BoxGeometry(0.78,0.05,0.05)` at ±π/4), translucent blades `0x9fb2c8` α0.8, red eye `0xff5533`
  (box 0.12×0.07×0.03), under-probe `0x1a2129`.
- **Skywing**: body/leg color `colors.body`, wing `colors.wing`, beak `0xd8a040`; body box
  0.3×0.2×0.7, two wings `1.1×0.025×0.3`, tail, beak.
- **Generic creature**: body `colors.body` (box w×h×d), legs `colors.legs` (4 hips + boxes), eyes
  `colors.eye` (spheres), optional fly wings. External GLB (`crab`/`strider`/`blob`) is loaded and
  **tinted** with `colors.body` via `ModelLib.get(name, size, {tint})`.
- **Villagers**: reuse `Humanoid.build` with `skin #e8c49a`, `hair #4a3018`, robe tint from
  `ROBE_TINTS = [0x8a6b4a, 0x6a7a8a, 0x7a8a5a, 0x9a6a5a, 0x6a5a8a]`, `pants #4a3c2e`, `boots #2e2620`,
  `glove #e8c49a`, `belt #c9963f`.

---

## 9. Animation

**`textures.js` contains no multi-frame animated textures.** There is no water/lava flow frame strip, no
frame count, no texture-frame timing. Animation is achieved at the material/shader level:

- **Water**: vertex displacement via shader uniform `uWTime` (`js/world.js`):
  ```
  if (normal.y > 0.5)
    transformed.y += sin(transformed.x*0.85 + uWTime*2.2)*0.035 + cos(transformed.z*0.7 + uWTime*1.6)*0.035
  ```
  The water **tint** is per-biome (`waterTint`, e.g. `0xff6a1a` for volcanic "lava lakes"); the water
  **texture** is the static 4-blue speckle from Section 3 (#11). There is **no separate lava tile** —
  lava is the same liquid system with an orange tint and a damage flag.
- **Conveyor belt**: UV scroll, not frames (`js/factory.js`):
  ```
  BELT_SPEED = 1.2   // offset units per second
  beltMat.map.offset.y = (beltMat.map.offset.y - dt * BELT_SPEED) % 1
  ```
  i.e. the `belt` tile (16×16, Section 3 #49) scrolls in −Y at **1.2 tiles/sec = 19.2 px/sec**, full
  loop every `1/1.2 ≈ 0.833 s`.
- **Furnace**: two static tiles `furnace_front` / `furnace_on` swapped as a state toggle (not frames).

---

## 10. Pixel-art render mode (low-res framebuffer + nearest upscale)

From `js/main.js` `resizeRenderer()` + `css/style.css`:

```
// renderer = new THREE.WebGLRenderer({ canvas, antialias: false })   // no MSAA
if (settings.style === 'pixel'):
    renderer.setPixelRatio(1)
    renderer.setSize( max(640, round(w*0.5)), max(360, round(h*0.5)), /*updateStyle=*/false )
    canvas.style.width  = w px   // full CSS window width
    canvas.style.height = h px
    // css/style.css line 15: canvas#game { image-rendering:pixelated }
else:
    // modern: low -> pixelRatio 0.75 ; mid -> min(dpr,1.5) ; high -> min(dpr,2)
    // high quality also: shadowMap.enabled=true (PCFSoft), ACESFilmicToneMapping, exposure 1.18,
    //                    CSS filter saturate(1.14) contrast(1.04)
```

- **Internal render target = 50 % of the window, floor-clamped to a minimum of 640×360.**
- **Upscale = CSS `image-rendering: pixelated`** (nearest-neighbor) to the full window size.
- **No MSAA** (`antialias:false`). When `style==='pixel'`, external models (creatures/ships) switch to
  nearest-neighbor texture sampling via `ModelLib.setPixel(true)` so their textures match the block look.
- In Rust/bevy this maps to: render the world to an off-screen target of size `(max(640, w/2),
  max(360, h/2))`, then blit to the window with a nearest sampler (or set the canvas CSS size larger with
  `image-rendering: pixelated` equivalent).

---

## 11. Color / alpha parsing notes for the Rust port

- 6-digit hex `#RRGGBB` → `(R,G,B,255)`.
- 8-digit hex `#RRGGBBAA` → `(R,G,B,A)`; AA is a hex byte (e.g. `cc`=204, `99`=153, `66`=102, `88`=136,
  `aa`=170, `80`=128, `dd`=221). Used by: glass shine, ingot streak, crystal shine, warp/antimatter strokes,
  gradient stops.
- `rgba(r,g,b,a)` / `hsla(h,s%,l%,a)` — convert to RGBA8 with the given alpha (alpha as 0..1 float →
  byte `round(a*255)`).
- `shade()` preserves alpha (only RGB multiplied).
- All procedural textures are produced in **sRGB byte space** (no gamma curve applied anywhere in the
  pipeline — the planet code explicitly comments that `pow(1/2.2)` was removed to avoid a space-vs-ground
  brightness mismatch).

---

## Appendix A — 2D value noise (`js/world.js makeNoise`)

```
function makeNoise(seed):
  rnd = mulberry32(seed)
  p = [0..255]; for i=255..1: j=floor(rnd()*(i+1)); swap(p[i], p[j])   // Fisher-Yates shuffle
  perm[512]: perm[i] = p[i & 255] for i in 0..511                        // doubled
  fade(t)   = t*t*t*(t*(t*6 - 15) + 10)                                 // quintic smoothstep
  grad2(h,x,y) = { x+y if h&3==0 ; -x+y if ==1 ; x-y if ==2 ; -x-y otherwise }
  n2(x,y):
    X=floor(x)&255; Y=floor(y)&255; x-=floor(x); y-=floor(y)
    u=fade(x); v=fade(y)
    a=perm[X]+Y; b=perm[X+1]+Y
    return lerp( lerp(grad2(perm[a],x,y), grad2(perm[b],x-1,y), u),
                 lerp(grad2(perm[a+1],x,y-1), grad2(perm[b+1],x-1,y-1), u), v )
  fbm2(x, y, oct=4, lac=2, gain=0.5):
    amp=1; f=1; sum=0; norm=0
    for i in 0..oct: sum += n2(x*f, y*f)*amp; norm += amp; amp*=gain; f*=lac
    return sum / norm
  return { n2, fbm2 }
```

`fbm2` returns values in roughly [−1,1] (normalized by the summed amplitudes). Used by the planet, sun,
and cloud textures in Section 6.

---

## Appendix B — Cloud shell texture (`js/space.js cloudShellTexture`, 256×128)

- `n = makeNoise(seed)`. Cloud-cover threshold per biome (`CLOUD`, lower = more clouds): `lush 0.55,
  ocean 0.52, fungal 0.5, murk 0.42, alien 0.4, hive 0.38, redmoss 0.36, frozen 0.44, crystal 0.4,
  salt 0.3, ferrous 0.34, obsidian 0.28, amber 0.24, desert 0.2, volcanic 0.16, ashen 0.12` (default 0.4).
- Per pixel:
  ```
  t0 = px/256
  v1 = fbm2(px*0.05, py*0.08, 4)*0.5+0.5
  v2 = fbm2((px-256)*0.05, py*0.08, 4)*0.5+0.5
  v  = v1*(1-t0) + v2*t0                          // longitude cross-fade (seamless)
  if stormy (ferrous|murk): v += sin(px/256*π*6 + fbm2(px*0.01,py*0.05,2)*2)*0.16
  a  = smoothstep(v, lo, lo+0.24)
  RGBA = (255,255,255, round(a*235))
  ```
- Rendered as a transparent white shell (`opacity 0.85`, `depthWrite false`) around the planet sphere.

---

## Appendix C — Station name/terminal screens (non-pixel, for completeness)

`space.js` also draws small text-canvases (`256×128` market ticker, `256×64` station sign) with canvas
text APIs; they are not pixel-art and are excluded from the pixel-texture port. Skip unless you need the
UI screens.

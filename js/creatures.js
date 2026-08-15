/* ============================================================
   STARFORGE - creatures.js
   体素生物 + 精模生物：生成 / 漫游 / 跟随地形 / 跳跃
   · 精模（GLB）播放骨骼动画（Walk/Idle/Death），四肢随行走摆动
   · 体素回退模型腿关节在髋部旋转摆动
   · AI：锚定领地（野生生物守巢、村民守村）、矮跳不爬树、
     前方障碍转向（不再穿树穿墙）、不朝玩家聚集
   · Minecraft（Java 版）式生成与持久化：
     - 世界生成：首次踏入区域时按确定性几率建立「兽群」（≈ 区块生成生物）
     - 自然生成的生物一旦生成永不消失（被动生物不 despawm）：
       距玩家 >128m 时只是卸载休眠（保留位置/血量），回来时原样重载
       → 可以围栏圈养、建设农场
     - 生成环 24~128m + 活跃数量上限（mob cap）+ 被杀后周期性补足
     - 淡入/淡出过渡——生物既不密集也不会当着玩家的面消失/刷出
   ============================================================ */
'use strict';

const Creatures = (() => {
  let group = null, list = [];
  let vGroup = null, villagers = [];        // 村民（独立分组：不参与刷新清理，不可被攻击）
  const dying = [];                         // 播放死亡动画中的精模生物
  const spawnedVillages = new Set();
  // ---------- 遗迹守卫（敌对飞行无人机）与天空浮翼（环境点缀）----------
  const ruinGuards = new Map();             // 'x,z' -> { alive:[Group], dead:n }
  const skyFlock = [];                      // 高空盘旋的浮翼群（纯环境，不参与兽群/存档/联机同步）
  let skyTimer = 10;
  // ---------- Minecraft 式（Java 版）生成与持久化 ----------
  const cellStates = new Map();    // 'cx,cz' -> { cx, cz, cands: [候选出生点], mask: 已占用位图, herdCount: 引用本格的兽群数 }
  const herds = new Map();         // nid -> 兽群记录（自然生成生物的世界状态：生成后永不消失，仅卸载/重载）
  const removedMasks = new Map();  // 'cx,cz' -> 被杀候选位图（存档持久化：被杀动物不复活，读档后同样不重生）
  const registerQueue = [];        // 待注册候选细胞（分帧补齐，避免集中生成地形卡顿）
  const fadingOut = [];            // 淡出中的生物（已移出活跃列表，只做视觉收尾）
  let lastCenter = null;           // 玩家当前网格（快速路径）
  let lastInfoType = null;         // 生态动物类型变化 → 重建
  let tickFrame = 0;               // 帧计数：远处生物降频 tick
  let spawnTimer = 0;              // 周期性生成计时器
  let pruneFrame = 0;              // 候选细胞回收帧计数
  const CRE_CAP = 16;              // 活跃生物上限（安全阀；正常密度下不会触发）
  const HERD_CHANCE = 0.18;        // 首次踏入一格时建立兽群的几率（≈ MC 区块生成生物的 1/10）
  const TARGET_DENSITY = 12;       // 玩家 128m 范围内兽群数量目标（低于此值才周期补足 → 被杀后缓慢恢复）
  const SPAWN_MIN = 24;            // 生成环内径：距玩家 < 24m 不生成（Minecraft 同款规则）
  const SPAWN_MAX = 128;           // 生成环外径（Minecraft 同款 128m）
  const UNLOAD_D = 128;            // 距玩家 > 128m：兽群卸载休眠（保留位置/血量，不删除）
  const RELOAD_D = 96;             // 距玩家 < 96m：休眠兽群重载（迟滞带，避免边界抖动）
  const SPAWN_INTERVAL = 1.2;      // 周期生成间隔（秒），每次最多补 1 个兽群
  const FADE_IN_T = 1.0, FADE_OUT_T = 0.8;
  function easeInOut(t){ return t <= 0 ? 0 : t >= 1 ? 1 : t * t * (3 - 2 * t); }

  // ---------- 联机：确定性 ID / 批次 ----------
  // 生物批次按 24m 网格生成，种子 = 世界种子+网格坐标 → 同网格的玩家看到同一批生物
  const CRE_CELL = 24;
  function batchSeedOf(cx, cz){
    let h = (World.seed ^ 0xC7EA5) >>> 0;
    h = Math.imul(h ^ cx, 374761393);
    h = Math.imul(h ^ cz, 668265263);
    h = (h ^ (h >>> 13)) >>> 0;
    return h;
  }
  function creatureNid(cx, cz, i){ return batchSeedOf(cx, cz) * 64 + i; }
  function villagerNid(vx, vz, i){
    let h = (World.seed ^ 0x7A9E1) >>> 0;
    h = Math.imul(h ^ vx, 374761393);
    h = Math.imul(h ^ vz, 668265263);
    h = Math.imul(h ^ i, 2246822519);
    h = (h ^ (h >>> 13)) >>> 0;
    return h;
  }

  // ---------- 联机：远端快照（跨客户端生物对齐）----------
  const ghosts = new Map();     // nid -> {g, tgt, last}（其他玩家批次的“投影”生物：纯视觉，不可交互）

  // 外部精模映射（CC0）：类型 → 模型名/朝向修正（使模型前方 = -Z）
  const GLB_MAP = {
    crab:    { name: 'crab', yaw: Math.PI },
    strider: { name: 'strider', yaw: Math.PI },
    blob:    { name: 'blob', yaw: 0 },
  };

  // ---------- 模型构建 ----------
  // 遗迹守卫：体素四旋翼无人机（暗色机体 + 红色独眼 + 旋转桨叶）
  function buildSentinel(){
    const g = new THREE.Group();
    const bodyM = new THREE.MeshLambertMaterial({ color: 0x2c333f });
    const darkM = new THREE.MeshLambertMaterial({ color: 0x1a2129 });
    const eyeM = new THREE.MeshBasicMaterial({ color: 0xff5533 });
    const body = new THREE.Mesh(new THREE.BoxGeometry(0.5, 0.26, 0.5), bodyM);
    g.add(body);
    // 两条交叉旋翼臂
    const armGeo = new THREE.BoxGeometry(0.78, 0.05, 0.05);
    for (const ang of [Math.PI / 4, -Math.PI / 4]){
      const arm = new THREE.Mesh(armGeo, darkM);
      arm.position.y = 0.05;
      arm.rotation.y = ang;
      g.add(arm);
    }
    // 四片半透明桨叶（动画：旋转）
    const rotors = new THREE.Group();
    const bladeM = new THREE.MeshBasicMaterial({ color: 0x9fb2c8, transparent: true, opacity: 0.8 });
    for (const [sx, sz] of [[-0.33,-0.33],[0.33,-0.33],[-0.33,0.33],[0.33,0.33]]){
      const blade = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.014, 0.06), bladeM);
      blade.position.set(sx, 0.13, sz);
      rotors.add(blade);
    }
    g.add(rotors);
    g.userData.rotors = rotors;
    // 红色独眼 + 底部探头
    const eye = new THREE.Mesh(new THREE.BoxGeometry(0.12, 0.07, 0.03), eyeM);
    eye.position.set(0, 0.03, -0.26);
    g.add(eye);
    const under = new THREE.Mesh(new THREE.BoxGeometry(0.14, 0.16, 0.1), darkM);
    under.position.set(0, -0.21, 0);
    g.add(under);
    return g;
  }
  // 天空浮翼：体素滑翔生物（双翼 + 尾翼）
  function buildSkywing(colors){
    const g = new THREE.Group();
    const bodyM = new THREE.MeshLambertMaterial({ color: colors.body });
    const wingM = new THREE.MeshLambertMaterial({ color: colors.wing });
    const body = new THREE.Mesh(new THREE.BoxGeometry(0.3, 0.2, 0.7), bodyM);
    g.add(body);
    for (const s of [-1, 1]){
      const wing = new THREE.Mesh(new THREE.BoxGeometry(1.1, 0.025, 0.3), wingM);
      wing.position.set(s * 0.42, 0.04, -0.05);
      g.add(wing);
    }
    const tail = new THREE.Mesh(new THREE.BoxGeometry(0.26, 0.05, 0.14), wingM);
    tail.position.set(0, 0.04, 0.38);
    g.add(tail);
    const beak = new THREE.Mesh(new THREE.BoxGeometry(0.06, 0.05, 0.14), new THREE.MeshLambertMaterial({ color: 0xd8a040 }));
    beak.position.set(0, -0.02, -0.4);
    g.add(beak);
    return g;
  }
  function buildCreature(typeDef, colors, typeKey){
    // 优先使用外部模型（按生态色染色），失败回退程序化体素生物
    const mm = GLB_MAP[typeKey];
    if (mm && window.ModelLib){
      const size = Math.max(typeDef.w, typeDef.h, typeDef.d) * 2.2;
      const glb = ModelLib.get(mm.name, size, { tint: colors.body, yaw: mm.yaw });
      if (glb){
        glb.userData.isGlb = true;
        return glb;
      }
    }
    const g = new THREE.Group();
    const { w, h, d } = typeDef;
    const bodyM = new THREE.MeshLambertMaterial({ color: colors.body });
    const legM = new THREE.MeshLambertMaterial({ color: colors.legs });
    const eyeM = new THREE.MeshBasicMaterial({ color: colors.eye });
    // 躯干
    const body = new THREE.Mesh(new THREE.BoxGeometry(w, h, d), bodyM);
    g.add(body);
    // 飞行型：无腿，加一对侧翼（children[1..2]，动画拍打）
    if (typeDef.fly){
      for (const s of [-1, 1]){
        const wing = new THREE.Mesh(new THREE.BoxGeometry(w * 1.9, 0.03, d * 0.55), legM);
        wing.position.set(s * w * 0.95, 0.02, 0);
        g.add(wing);
      }
      return g;
    }
    // 腿（髋关节在腿顶：绕髋部前后摆动）
    for (const [lx, lz] of [[-w * 0.35, -d * 0.4], [w * 0.35, -d * 0.4], [-w * 0.35, d * 0.4], [w * 0.35, d * 0.4]]){
      const hip = new THREE.Group();
      hip.position.set(lx, -h / 2 + h * 0.225, lz);
      const legGeo = new THREE.BoxGeometry(w * 0.14, h * 0.45, w * 0.14);
      legGeo.translate(0, -h * 0.225, 0);
      const leg = new THREE.Mesh(legGeo, legM);
      hip.add(leg);
      g.add(hip);
    }
    // 头/眼
    if (typeDef.headW > 0){
      const head = new THREE.Mesh(new THREE.BoxGeometry(typeDef.headW, h * 0.4, typeDef.headW), bodyM);
      head.position.set(0, h * 0.3, -d / 2);
      g.add(head);
      // 双眼
      for (const ex of [-typeDef.headW * 0.2, typeDef.headW * 0.2]){
        const eye = new THREE.Mesh(new THREE.SphereGeometry(typeDef.headW * 0.15, 4, 4), eyeM);
        eye.position.set(ex, h * 0.38, -d / 2 - typeDef.headW * 0.2);
        g.add(eye);
      }
    }
    return g;
  }

  function init(scene){
    group = new THREE.Group();
    scene.add(group);
    list = [];
    vGroup = new THREE.Group();
    scene.add(vGroup);
    villagers = [];
    dying.length = 0;
    spawnedVillages.clear();
    ghosts.clear();
    skyFlock.length = 0;
    ruinGuards.clear();
    cellStates.clear();
    herds.clear();
    removedMasks.clear();
    registerQueue.length = 0;
    fadingOut.length = 0;
    lastCenter = null;
    lastInfoType = null;
    spawnTimer = 0;
  }

  // ---------- 村民：统一人形管线（Humanoid），四肢关节可动 ----------
  const VILLAGER_NAMES = ['老农全叔', '铁匠芒果', '采药人芸', '守望者白', '小豆豆', '织工兰', '猎户岩', '酿蜜人蓬', '陶匠小满', '游方商乌拉'];
  const ROBE_TINTS = [0x8a6b4a, 0x6a7a8a, 0x7a8a5a, 0x9a6a5a, 0x6a5a8a];
  const _vilCache = {};   // 上衣色 → 已建模板
  function buildVillager(idx){
    const tint = ROBE_TINTS[idx % ROBE_TINTS.length];
    const key = tint.toString(16);
    if (!_vilCache[key]){
      _vilCache[key] = Humanoid.build({
        skin: '#e8c49a', hair: '#4a3018', hairStyle: 'short',
        suit: '#' + tint.toString(16).padStart(6, '0'),
        pants: '#4a3c2e', boots: '#2e2620', glove: '#e8c49a', belt: '#c9963f',
      });
    }
    return _vilCache[key].clone(true);   // 克隆时几何共享（模板永不被 dispose；关节由 spawnVillages 按孩子顺序重建）
  }
  // 克隆树的关节组按模板构建顺序还原（head/torso/armL/armR/legL/legR 依次挂在根上）
  const JOINT_ORDER = ['head', 'torso', 'armL', 'armR', 'legL', 'legR'];
  function rigFromClone(g){
    const rig = {};
    JOINT_ORDER.forEach((k, i) => { if (g.children[i]) rig[k] = g.children[i]; });
    return rig;
  }
  function buildVillagerTemplate(){
    // 兼容旧调用：返回一个默认模板
    return buildVillager(0);
  }
  function spawnVillages(plyPos){
    if (!vGroup || !World.structures) return;
    for (const st of World.structures){
      if (st.type !== 'village') continue;
      const key = st.x + ',' + st.z;
      if (spawnedVillages.has(key)) continue;
      const dx = plyPos.x - st.x, dz = plyPos.z - st.z;
      if (dx * dx + dz * dz > 150 * 150) continue;
      spawnedVillages.add(key);
      const rnd = mulberry32((st.x * 31 + st.z * 131) >>> 0);
      const n = 3 + ((rnd() * 3) | 0);
      for (let i = 0; i < n; i++){
        const g = buildVillager((rnd() * 10) | 0);
        // 出生点：村心附近 5 格内，且不落在树木上
        let vx = st.x, vz = st.z, gy = World.topAt(Math.floor(vx), Math.floor(vz));
        for (let t = 0; t < 6; t++){
          vx = st.x + (rnd() - 0.5) * 10; vz = st.z + (rnd() - 0.5) * 10;
          gy = World.topAt(Math.floor(vx), Math.floor(vz));
          const dd = World.getDef(Math.floor(vx), gy, Math.floor(vz));
          if (!dd || (!dd.liquid && dd.key !== 'log' && dd.key !== 'leaves')) break;
        }
        const grig = rigFromClone(g);
        g.position.set(vx, gy + 1 + 0.02, vz);
        g.scale.setScalar(0.01);                       // 淡入：不贴着村庄 150m 环当面刷出
        const nid = villagerNid(st.x, st.z, i);
        g.userData = {
          villager: true, isGlb: true, rig: grig,
          nid, rnd: mulberry32(nid),           // 联机：确定性行为随机源（跨客户端一致）
          name: VILLAGER_NAMES[(rnd() * VILLAGER_NAMES.length) | 0],
          home: { x: st.x, z: st.z },
          speed: 0.6 + rnd() * 0.4,
          dir: rnd() * Math.PI * 2,
          state: 'idle', timer: 1 + rnd() * 4,
          jumpVel: 0, onGround: true,
          typeDef: { speed: 0.8, jump: false, h: 1.7 }, animT: rnd() * 10, foot: 0.02,
          spawnT: FADE_IN_T,
        };
        vGroup.add(g);
        villagers.push(g);
      }
    }
  }
  function nearestVillager(pos, maxD){
    let best = null, bestD = maxD || 3.6;
    for (const g of villagers){
      const d = g.position.distanceTo(pos);
      if (d < bestD){ bestD = d; best = g; }
    }
    return best ? { g: best, dist: bestD } : null;
  }

  // ---------- 遗迹守卫：敌对飞行无人机（营地式重生：离开 150m 再回来重新部署）----------
  function sentinelNid(rx, rz, i){
    let h = (World.seed ^ 0x5E171) >>> 0;
    h = Math.imul(h ^ rx, 374761393);
    h = Math.imul(h ^ rz, 668265263);
    h = Math.imul(h ^ i, 2246822519);
    h = (h ^ (h >>> 13)) >>> 0;
    return h;
  }
  function materializeSentinel(x, z, nid, rnd, ruinKey){
    const g = buildSentinel();
    const rotors = g.userData.rotors;
    const gy = World.topAt(Math.floor(x), Math.floor(z));
    g.position.set(x, gy + 1 + 5, z);
    g.userData = {
      nid, rnd,
      sentinel: true, hostile: true, aggroR: 16, sentinelKey: ruinKey,
      typeDef: CREATURE_TYPES.sentinel,
      hp: CREATURE_TYPES.sentinel.hp || 10,
      hoverAlt: 4.5 + rnd() * 1.5,
      dir: rnd() * Math.PI * 2,
      state: 'idle', timer: 2 + rnd() * 3,
      jumpVel: 0, onGround: false, fly: true,
      speed: 2.4, radius: 0.55, foot: 0.5,
      animT: rnd() * 10,
      home: { x, z },
      atkCd: 0, backoff: 0, aggro: false,
      rotors,
      spawnT: FADE_IN_T,
    };
    g.scale.setScalar(0.01);
    group.add(g);
    list.push(g);
    return g;
  }
  function spawnRuinGuards(plyPos){
    if (!group || !World.structures) return;
    for (const st of World.structures){
      if (st.type !== 'ruin') continue;
      const key = st.x + ',' + st.z;
      const dx = plyPos.x - st.x, dz = plyPos.z - st.z;
      const d2 = dx * dx + dz * dz;
      if (d2 < 60 * 60){
        let rec = ruinGuards.get(key);
        if (!rec){ rec = { alive: [], dead: 0 }; ruinGuards.set(key, rec); }
        rec.alive = rec.alive.filter(g => list.includes(g));   // 清掉已死亡/已卸载的引用
        // 营地式重生：被清剿后（dead>0）玩家停留在区域内不复活；离开 150m 再回来重新部署
        if (!rec.alive.length && rec.dead === 0){
          const rnd = mulberry32(((st.x * 7 + st.z * 13) ^ 0x5E171) >>> 0);
          const n = 1 + ((rnd() * 2) | 0);   // 1~2 台
          for (let i = 0; i < n; i++){
            const ang = rnd() * Math.PI * 2;
            const gx = st.x + Math.cos(ang) * (4 + rnd() * 6);
            const gz = st.z + Math.sin(ang) * (4 + rnd() * 6);
            rec.alive.push(materializeSentinel(gx, gz, sentinelNid(st.x, st.z, i), mulberry32(sentinelNid(st.x, st.z, i)), key));
          }
        }
      } else if (d2 > 150 * 150){
        const rec = ruinGuards.get(key);
        if (rec){
          rec.dead = 0;   // 离开区域：营地重置，下次接近重新部署
          rec.alive = rec.alive.filter(g => list.includes(g));
        }
      }
    }
  }

  // ---------- 天空浮翼群：高空环境点缀（纯本地，不联机同步）----------
  function spawnSkyFlock(plyPos, sky){
    const n = 3 + ((Math.random() * 3) | 0);
    const ang0 = Math.random() * Math.PI * 2;
    for (let i = 0; i < n; i++){
      const g = buildSkywing(sky);
      const ang = ang0 + (i - n / 2) * 0.35;
      const dist = 26 + Math.random() * 30;
      const gy = World.topAt(Math.floor(plyPos.x), Math.floor(plyPos.z));
      g.position.set(
        plyPos.x + Math.cos(ang) * dist,
        gy + 1 + 26 + Math.random() * 14,
        plyPos.z + Math.sin(ang) * dist);
      g.userData = {
        skywing: true, fly: true, typeDef: CREATURE_TYPES.skywing,
        dir: ang0 + Math.PI / 2 + (Math.random() - 0.5) * 0.8,
        speed: 2.2 + Math.random() * 1.2,
        animT: Math.random() * 10,
        hoverAlt: 24 + Math.random() * 12,
        life: 75 + Math.random() * 45,
        wings: [g.children[1], g.children[2]],
        radius: 0.5,
      };
      group.add(g);
      skyFlock.push(g);
    }
  }
  function updateSky(dt, plyPos, biome){
    const sky = biome && biome.sky;
    if (!sky){
      // 生态无高空生物（或换到无天空群的星球）：清空旧群
      if (skyFlock.length){
        for (const g of skyFlock){ group.remove(g); disposeObject3D(g, { skipGeo: true, skipTex: true, skipMat: true }); }
        skyFlock.length = 0;
      }
      return;
    }
    skyTimer -= dt;
    if (skyTimer <= 0){
      skyTimer = 30 + Math.random() * 40;
      if (skyFlock.length < 14) spawnSkyFlock(plyPos, sky);
    }
    for (let i = skyFlock.length - 1; i >= 0; i--){
      const g = skyFlock[i];
      const u = g.userData;
      u.animT += dt;
      u.life -= dt;
      if (u.life <= 0){
        group.remove(g);
        disposeObject3D(g, { skipGeo: true, skipTex: true, skipMat: true });
        skyFlock.splice(i, 1);
        continue;
      }
      // 缓慢盘旋 + 轻微转向漂移
      u.dir += Math.sin(u.animT * 0.35) * 0.22 * dt;
      g.position.x += Math.cos(u.dir) * u.speed * dt;
      g.position.z += Math.sin(u.dir) * u.speed * dt;
      const gy = topAtRo(Math.floor(g.position.x), Math.floor(g.position.z));
      if (gy !== null){
        const targetY = gy + 1 + u.hoverAlt + Math.sin(u.animT * 0.8) * 1.2;
        g.position.y += (targetY - g.position.y) * Math.min(1, dt * 1.5);
      }
      g.rotation.y = -u.dir - Math.PI / 2;
      g.rotation.z = Math.sin(u.animT * 0.8) * 0.12;
      // 拍翼
      const flap = Math.sin(u.animT * 5) * 0.5;
      for (const w of u.wings){ if (w) w.rotation.z = flap * (w.position.x > 0 ? -1 : 1); }
      // 距离终点渐隐
      if (u.life < 4) g.scale.setScalar(Math.max(0.01, u.life / 4));
      else g.scale.setScalar(Math.min(1, g.scale.x + dt * 3));
    }
  }

  // 地形判断：地面列顶是否是树木/液体（生物不生成在树上/水里）
  function solidDefAt(x, y, z){ return World.getDef(Math.floor(x), Math.floor(y), Math.floor(z)); }

  // 每帧 AI 地形查询：只读（未加载区块返回 null，绝不触发生成）+ 按帧缓存同一列结果。
  // 此前每只生物每帧 2~4 次 topAt，每次都可能 genChunk + 全高扫描——AI 会把地形生成拖进主循环造成卡顿。
  const topCache = new Map();   // 'x,z' -> { y, frame }
  function topAtRo(x, z){
    x = Math.floor(x); z = Math.floor(z);
    const k = x + ',' + z;
    const e = topCache.get(k);
    if (e && e.frame === tickFrame) return e.y;
    if (topCache.size > 8192) topCache.clear();
    const y = World.topAtNoGen(x, z);
    topCache.set(k, { y, frame: tickFrame });
    return y;
  }

  // plyPos: Vector3, biome: 星球生态对象
  function update(dt, plyPos, biome){
    if (!group) return;
    spawnVillages(plyPos);   // 靠近村庄时生成村民（每村一次）
    spawnRuinGuards(plyPos); // 靠近遗迹时部署守卫无人机（营地式重生）
    updateSky(dt, plyPos, biome);   // 高空浮翼群（环境点缀）
    const info = biome.animal;
    if (!info) return;

    // 生态动物类型变化（换星球）→ 旧生物全部清掉重建
    if (lastInfoType !== info.type){
      lastInfoType = info.type;
      clearBatches();
    }

    spawnTimer += dt;
    const cx = Math.floor(plyPos.x / CRE_CELL), cz = Math.floor(plyPos.z / CRE_CELL);
    const key = cx + ',' + cz;
    if (lastCenter !== key){
      lastCenter = key;
      // 本格候选立即注册（世界生成式兽群掷骰），7×7 邻域其余细胞入队分帧注册
      registerCell(cx, cz, info);
      for (let dx = -3; dx <= 3; dx++){
        for (let dz = -3; dz <= 3; dz++){
          if (dx === 0 && dz === 0) continue;
          const k = (cx + dx) + ',' + (cz + dz);
          if (!cellStates.has(k) && !registerQueue.includes(k)) registerQueue.push(k);
        }
      }
    }

    // 分帧注册候选细胞（每帧最多 3 个，避免集中生成地形造成卡顿）
    for (let n = 0; n < 3 && registerQueue.length; n++){
      const k = registerQueue.shift();
      const parts = k.split(',');
      registerCell(+parts[0], +parts[1], info);
    }

    // 休眠兽群重载 / 首次物化（每帧最多 3 个，避免集中卡顿）
    materializePass(plyPos, info);

    // Minecraft 式周期补足：128m 内兽群少于目标密度时，在生成环 24~128m 内补 1 个
    //（玩家击杀动物后缓慢恢复；兽群本身永不消失，只增不减直到被击杀）
    if (spawnTimer >= SPAWN_INTERVAL){
      spawnTimer = 0;
      spawnCycle(plyPos, info);
    }

    // 定期回收远处已无兽群引用的候选细胞（返回时按确定性种子重建）
    pruneFrame = (pruneFrame + 1) & 63;
    if (pruneFrame === 0) pruneCells(plyPos);
  }

  // 注册一个候选细胞：出生点与行为参数全部确定性（联机跨客户端一致），
  // 并做一次「世界生成式」兽群掷骰（≈ Minecraft 区块生成生物：被动动物在此诞生）
  function registerCell(cx, cz, info){
    const key = cx + ',' + cz;
    if (cellStates.has(key)) return;
    const typeDef = CREATURE_TYPES[info.type] || CREATURE_TYPES.strider;
    const rnd = mulberry32(batchSeedOf(cx, cz));
    const ccx = cx * CRE_CELL + CRE_CELL / 2, ccz = cz * CRE_CELL + CRE_CELL / 2;
    const cands = [];
    for (let i = 0; i < Math.min(info.count, 22); i++){
      // 出生点选择：细胞中心 12~92 格随机环（确定性），避开水体与树木顶端
      let wx = 0, wz = 0, gy = 0, ok = false;
      for (let t = 0; t < 8 && !ok; t++){
        const ang = rnd() * Math.PI * 2;
        const dist = 12 + rnd() * 80;
        wx = ccx + Math.cos(ang) * dist; wz = ccz + Math.sin(ang) * dist;
        gy = World.topAt(Math.floor(wx), Math.floor(wz));
        const dd = solidDefAt(wx, gy, wz);
        ok = !!dd && !dd.liquid && dd.key !== 'log' && dd.key !== 'leaves';
      }
      // 确定性行为参数（RNG 消耗顺序与旧版一致，保证跨客户端对齐）
      const speed = typeDef.speed * (0.5 + rnd());
      const dir = rnd() * Math.PI * 2;
      const timer = 1 + rnd() * 3;
      const animT = rnd() * 10;
      cands.push({ i, x: wx, z: wz, gy, speed, dir, timer, animT });
    }
    // 世界生成式兽群（确定性掷骰：跨客户端一致）；被杀候选（removed 掩码）与
    // 存档恢复的既有兽群均保持占用，不重复生成
    const roll = rnd();
    const herdIdx = (rnd() * cands.length) | 0;
    const rm = removedMasks.get(key) || 0;
    const st = { cx, cz, cands, mask: rm, herdCount: 0 };
    cellStates.set(key, st);
    if (cands.length && roll < HERD_CHANCE && !(rm & (1 << herdIdx))){
      const nid = creatureNid(cx, cz, herdIdx);
      const existing = herds.get(nid);
      if (existing){
        // 存档恢复的兽群：用确定性参数补全行为数据，并标记占用
        st.mask |= (1 << herdIdx);
        const c = cands[herdIdx];
        existing.speed = c.speed; existing.dir = c.dir; existing.timer = c.timer; existing.animT = c.animT;
      } else {
        createHerd(st, cands[herdIdx]);
      }
    }
  }

  // ---------- 存档序列化（兽群世界状态随世界记录落盘）----------
  // herds:  [cx, cz, candIdx, x×10, z×10, hp, homeX×10, homeZ×10]
  // removed: ['cx,cz', 被杀候选位图]
  function serialize(){
    const herdsArr = [];
    for (const h of herds.values()){
      if (h.g){ h.x = h.g.position.x; h.z = h.g.position.z; h.hp = h.g.userData.hp || 4; }
      herdsArr.push([h.cx, h.cz, h.candIdx, Math.round(h.x * 10), Math.round(h.z * 10), Math.round(h.hp || 0), Math.round(h.homeX * 10), Math.round(h.homeZ * 10)]);
    }
    const removed = [];
    for (const [k, mask] of removedMasks) if (mask) removed.push([k, mask]);
    return { herds: herdsArr, removed };
  }
  // 读档恢复：兽群（位置/血量/领地）与击杀记录全部还原；被杀动物不会复活
  function restore(data){
    herds.clear();
    removedMasks.clear();
    if (!data || typeof data !== 'object') return;
    if (Array.isArray(data.removed)){
      for (const e of data.removed){
        if (!Array.isArray(e) || e.length < 2 || typeof e[0] !== 'string' || !Number.isInteger(e[1])) continue;
        removedMasks.set(e[0], e[1] | 0);
      }
    }
    if (Array.isArray(data.herds)){
      for (const e of data.herds){
        if (!Array.isArray(e) || e.length < 8) continue;
        if (!Number.isInteger(e[0]) || !Number.isInteger(e[1]) || !Number.isInteger(e[2])) continue;
        const x = Number.isFinite(e[3]) ? e[3] / 10 : 0, z = Number.isFinite(e[4]) ? e[4] / 10 : 0;
        const hx = Number.isFinite(e[6]) ? e[6] / 10 : x, hz = Number.isFinite(e[7]) ? e[7] / 10 : z;
        const nid = creatureNid(e[0], e[1], e[2]);
        herds.set(nid, {
          nid, cx: e[0], cz: e[1], candIdx: e[2],
          x, z, hp: e[5] | 0, homeX: hx, homeZ: hz, g: null, first: false,
          speed: 1, dir: 0, timer: 1, animT: Math.random() * 10,   // 注册时由确定性候选补全
        });
      }
    }
  }

  // 建立兽群记录：自然生成的生物一旦生成永不消失（Java 版被动生物不 despawm），
  // 距玩家 >128m 只是卸载休眠，回来时原样重载 → 可围栏圈养、建设农场
  function createHerd(st, c){
    const nid = creatureNid(st.cx, st.cz, c.i);
    herds.set(nid, {
      nid, cx: st.cx, cz: st.cz, candIdx: c.i,
      x: c.x, z: c.z, homeX: c.x, homeZ: c.z, hp: 4, g: null, first: true,
      speed: c.speed, dir: c.dir, timer: c.timer, animT: c.animT,
    });
    st.mask |= (1 << c.i);
    st.herdCount++;
    return nid;
  }

  // 物化兽群（首次生成或卸载后重载）：重校验地形，淡入出场，保留血量
  function materializeHerd(herd, info){
    const gy = World.topAt(Math.floor(herd.x), Math.floor(herd.z));
    const dd = solidDefAt(herd.x, gy, herd.z);
    if (!dd || dd.liquid || dd.key === 'log' || dd.key === 'leaves') return false;   // 地形被破坏 → 保持休眠，等恢复
    const typeDef = CREATURE_TYPES[info.type] || CREATURE_TYPES.strider;
    const g = buildCreature(typeDef, { body: info.body, legs: info.legs, eye: info.eye }, info.type);
    // 贴地偏移：模型原点(躯干中心)到最低点（腿底）的距离，站在方块顶面(gy+1)上
    const foot = footOffset(g, typeDef);
    g.position.set(herd.x, gy + 1 + foot, herd.z);
    const glbClips = g.userData.clips || null;
    g.userData = {
      nid: herd.nid, rnd: mulberry32(herd.nid),   // 联机：确定性行为随机源（跨客户端一致）
      speed: herd.speed,
      dir: herd.dir,
      state: 'idle', timer: herd.timer,
      jumpVel: 0, onGround: true, jumpCd: 0,
      typeDef, animT: herd.animT, foot,
      hp: herd.hp || 4, isGlb: !!g.userData.isGlb,
      clips: glbClips,
      home: { x: herd.homeX, z: herd.homeZ },     // 领地锚点：兽群出生地，不向玩家聚集
      radius: Math.max(0.55, Math.max(typeDef.w, typeDef.h, typeDef.d) * 1.3),
      herd, spawnT: FADE_IN_T,                    // 淡入计时（0.01 → 1）
    };
    g.scale.setScalar(0.01);
    group.add(g);
    list.push(g);
    herd.g = g;
    herd.first = false;
    // 若远端投影（其他玩家快照）仍在 → 本地实体已出现，移除投影避免双影
    const gh = ghosts.get(herd.nid);
    if (gh){
      disposeObject3D(gh.g, { skipGeo: !!gh.g.userData.isGlb, skipTex: true, skipMat: !!gh.g.userData.isGlb });
      group.remove(gh.g);
      ghosts.delete(herd.nid);
    }
    return true;
  }

  // 卸载兽群：保留世界状态（位置/血量），距玩家回到 96m 内时原样重载
  function unloadHerd(g){
    const u = g.userData, herd = u.herd;
    if (herd){
      herd.x = g.position.x; herd.z = g.position.z;
      herd.hp = u.hp || 4;
      herd.g = null;
      u.herd = null;
    }
  }

  // 删除兽群记录（击杀后：动物真的死了，不会复活；周期补足会另建新兽群）
  function removeHerdRecord(herd){
    herds.delete(herd.nid);
    const key = herd.cx + ',' + herd.cz;
    // 被杀候选永久移除（Java 式）：占用位保持、并记入 removed 掩码随存档持久化
    removedMasks.set(key, (removedMasks.get(key) || 0) | (1 << herd.candIdx));
    const st = cellStates.get(key);
    if (st){
      st.herdCount = Math.max(0, st.herdCount - 1);
      st.mask |= (1 << herd.candIdx);
    }
  }
  function removeHerdObject(g){
    const u = g.userData, herd = u.herd;
    u.herd = null;
    if (herd){
      herd.g = null;
      removeHerdRecord(herd);
    }
  }

  // 休眠兽群的重载/首次物化扫描（距玩家 96m 内才物化，活跃上限内每帧最多 3 个）
  function materializePass(plyPos, info){
    const pcx = Math.floor(plyPos.x / CRE_CELL), pcz = Math.floor(plyPos.z / CRE_CELL);
    let budget = 3;
    for (const herd of herds.values()){
      if (budget <= 0) return;
      if (herd.g) continue;                                          // 已活跃
      if (Math.abs(herd.cx - pcx) > 6 || Math.abs(herd.cz - pcz) > 6) continue;   // 远格快速跳过
      const dx = herd.x - plyPos.x, dz = herd.z - plyPos.z;
      const d2 = dx * dx + dz * dz;
      if (d2 >= RELOAD_D * RELOAD_D) continue;                       // 尚未进入重载半径
      if (herd.first && d2 < SPAWN_MIN * SPAWN_MIN) continue;        // 首次生成：距玩家太近不物化（防当面刷出）
      if (list.length >= CRE_CAP) return;                            // 活跃上限安全阀（逐只检查，绝不超限）
      if (materializeHerd(herd, info)) budget--;
    }
  }

  // 周期补足：128m 内兽群少于目标密度时，在生成环 24~128m 内新建 1 个兽群
  function spawnCycle(plyPos, info){
    if (list.length >= CRE_CAP) return;
    const pcx = Math.floor(plyPos.x / CRE_CELL), pcz = Math.floor(plyPos.z / CRE_CELL);
    let local = 0;
    for (const herd of herds.values()){
      if (Math.abs(herd.cx - pcx) > 6 || Math.abs(herd.cz - pcz) > 6) continue;
      const dx = herd.x - plyPos.x, dz = herd.z - plyPos.z;
      if (dx * dx + dz * dz < UNLOAD_D * UNLOAD_D) local++;
    }
    if (local >= TARGET_DENSITY) return;
    const cells = [];
    for (const st of cellStates.values()){
      const dx = (st.cx * CRE_CELL + CRE_CELL / 2) - plyPos.x;
      const dz = (st.cz * CRE_CELL + CRE_CELL / 2) - plyPos.z;
      cells.push([dx * dx + dz * dz, st]);
    }
    cells.sort((a, b) => a[0] - b[0]);
    for (let ci = 0; ci < cells.length; ci++){
      const st = cells[ci][1];
      for (const c of st.cands){
        if (st.mask & (1 << c.i)) continue;   // 候选已被兽群占用
        const dx = c.x - plyPos.x, dz = c.z - plyPos.z;
        const d2 = dx * dx + dz * dz;
        if (d2 < SPAWN_MIN * SPAWN_MIN || d2 >= SPAWN_MAX * SPAWN_MAX) continue;
        const nid = createHerd(st, c);
        materializeHerd(herds.get(nid), info);
        return;
      }
    }
  }

  // 回收远处已无兽群引用的候选细胞（返回时按确定性种子重建）
  function pruneCells(plyPos){
    for (const [k, st] of cellStates){
      if (st.herdCount > 0) continue;
      const dx = (st.cx * CRE_CELL + CRE_CELL / 2) - plyPos.x;
      const dz = (st.cz * CRE_CELL + CRE_CELL / 2) - plyPos.z;
      if (dx * dx + dz * dz > 220 * 220) cellStates.delete(k);
    }
  }

  // 清空所有生物（换星球/换生态时调用）
  function clearBatches(){
    for (const g of list) disposeObject3D(g, { skipGeo: !!g.userData.isGlb, skipTex: true });
    for (const g of list) group.remove(g);
    for (const g of dying) disposeObject3D(g, { skipGeo: !!g.userData.isGlb, skipTex: true });
    for (const g of dying) group.remove(g);
    for (const f of fadingOut){
      disposeObject3D(f.g, { skipGeo: !!f.g.userData.isGlb, skipTex: true });
      group.remove(f.g);
    }
    list = [];
    dying.length = 0;
    fadingOut.length = 0;
    skyFlock.length = 0;
    ruinGuards.clear();
    cellStates.clear();
    herds.clear();
    removedMasks.clear();
    registerQueue.length = 0;
    lastCenter = null;
    spawnTimer = 0;
  }
  // 从活跃列表移除（击杀/淡出移除共用）
  function removeFromList(g){
    const i = list.indexOf(g);
    if (i >= 0) list.splice(i, 1);
  }
  // 模型原点到最低点的距离（含微小离地悬浮，避免脚底穿插）
  const _fBox = new THREE.Box3();
  function footOffset(g, typeDef){
    _fBox.setFromObject(g);
    if (isFinite(_fBox.min.y)) return -_fBox.min.y + 0.06;
    return typeDef.h * 0.75;
  }

  // ---------- 每帧更新 ----------
  function tick(dt, plyPos){
    tickFrame++;
    // 生物：淡入 / 距离淡出 / 远处降频
    for (let i = list.length - 1; i >= 0; i--){
      const g = list[i];
      const u = g.userData;
      // 淡入（出生时从 0.01 平滑放大到 1）
      if (u.spawnT !== undefined && u.spawnT > 0){
        u.spawnT -= dt;
        g.scale.setScalar(Math.max(0.01, easeInOut(1 - u.spawnT / FADE_IN_T)));
      }
      // 距玩家过远 → 淡出后卸载休眠（兽群不消失：保留位置/血量，回来时重载）
      const dx = g.position.x - plyPos.x, dz = g.position.z - plyPos.z;
      const d2 = dx * dx + dz * dz;
      if (d2 > UNLOAD_D * UNLOAD_D){
        removeFromList(g);
        fadingOut.push({ g, t: FADE_OUT_T });
        continue;
      }
      // 远处生物降频（每 4 帧一次，按 nid 错开）；近处与远端对齐保持全帧率
      if (!u.remote && d2 > 70 * 70 && ((tickFrame + (u.nid || 0)) & 3) !== 0) continue;
      tickOne(g, dt, plyPos);
    }
    // 淡出收尾：缩到最小后卸载休眠（兽群记录保留位置/血量，回来时重载）
    for (let i = fadingOut.length - 1; i >= 0; i--){
      const f = fadingOut[i];
      f.t -= dt;
      if (f.t <= 0){
        unloadHerd(f.g);
        disposeObject3D(f.g, { skipGeo: !!f.g.userData.isGlb, skipTex: true });
        group.remove(f.g);
        fadingOut.splice(i, 1);
      } else {
        f.g.scale.setScalar(Math.max(0.01, easeInOut(f.t / FADE_OUT_T)));
      }
    }
    for (const g of villagers){
      const u = g.userData;
      if (u.spawnT !== undefined && u.spawnT > 0){      // 村民淡入
        u.spawnT -= dt;
        g.scale.setScalar(Math.max(0.01, easeInOut(1 - u.spawnT / FADE_IN_T)));
      }
      tickOne(g, dt, plyPos);
    }
    // 死亡动画播完 → 移除
    for (let i = dying.length - 1; i >= 0; i--){
      const g = dying[i];
      const u = g.userData;
      u.dying.t -= dt;
      if (u.mixer) u.mixer.update(dt);
      if (u.dying.t <= 0){
        removeHerdObject(g);   // 死亡动画播完 → 兽群记录删除（被杀动物不复活）
        disposeObject3D(g, { skipGeo: !!u.isGlb, skipTex: true });
        group.remove(g);
        dying.splice(i, 1);
      }
    }
    // 联机投影生物：向快照位置平滑移动，4 秒无更新则移除
    const now = performance.now();
    for (const [nid, gh] of ghosts){
      // 淡入（不在玩家眼前凭空出现）
      if (gh.fade > 0){
        gh.fade -= dt;
        gh.g.scale.setScalar(Math.max(0.01, easeInOut(1 - gh.fade / FADE_IN_T)));
      }
      const k = Math.min(1, dt * 6);
      gh.g.position.x += (gh.tgt.x - gh.g.position.x) * k;
      gh.g.position.y += (gh.tgt.y - gh.g.position.y) * k;
      gh.g.position.z += (gh.tgt.z - gh.g.position.z) * k;
      gh.g.rotation.y = -gh.tgt.dir - Math.PI / 2;
      if (now - gh.last > 4000){
        disposeObject3D(gh.g, { skipGeo: !!gh.g.userData.isGlb, skipTex: true, skipMat: !!gh.g.userData.isGlb });
        group.remove(gh.g);
        ghosts.delete(nid);
      }
    }
  }

  // 前方通行检测：超过 1 格的高台（树干/高墙/崖壁）或身体高度有实心方块（穿树穿墙）→ 阻挡
  // 1 格以内的台阶允许直接走上（生物不会被地形卡死堆积）；树干列 topAt 是树冠（4~6 格高）→ 天然被挡
  function blockedAhead(u, nx, nz, curGy){
    const fx = Math.floor(nx), fz = Math.floor(nz);
    const newGy = topAtRo(fx, fz);
    if (newGy === null) return true;   // 目标区块未加载：视为阻挡（AI 绝不触发生成）
    const maxStep = 1.05;
    if (newGy > curGy + maxStep) return true;
    const bodyH = Math.max(1, (u.typeDef && u.typeDef.h) || 1.4);
    for (let y = newGy + 1; y <= newGy + Math.ceil(bodyH); y++){
      const d = World.getDef(fx, y, fz);
      if (d && d.id !== 0 && !d.cross && !d.liquid && !d.lowbox) return true;
    }
    return false;
  }
  // 方向障碍转向：随机折返，避免持续顶墙
  function dodge(u){
    u.dir += (u.rnd() < 0.5 ? 1 : -1) * (Math.PI * 0.55 + u.rnd() * 0.9);
  }

  // ---------- 飞行移动（无人机/浮翼）：悬浮高度保持 + 空中避障 + 敌对追击 ----------
  function flyMove(g, u, dt, plyPos){
    let moving = false;
    if (u.hostile){
      const dx = plyPos.x - g.position.x, dz = plyPos.z - g.position.z;
      const d = Math.hypot(dx, dz);
      if (d < u.aggroR && !u.aggro){
        u.aggro = true;
        u.dir = Math.atan2(dz, dx);
      }
      if (u.aggro && d > u.aggroR + 6){
        u.aggro = false;   // 玩家脱离警戒 → 回巢巡逻
      }
      if (u.aggro){
        // 追击：俯冲贴近（高度压低），命中后后撤再冲
        u.dir = Math.atan2(dz, dx);
        const spd = Math.min(u.speed * 2.6, 9);
        const back = u.backoff > 0;
        const mv = back ? -spd * 0.7 : spd;
        u.backoff -= dt;
        g.position.x += Math.cos(u.dir) * mv * dt;
        g.position.z += Math.sin(u.dir) * mv * dt;
        moving = true;
        if (!back && d < 1.15){
          u.atkCd -= dt;
          if (u.atkCd <= 0){
            u.atkCd = 1.15;
            u.backoff = 0.55;
            if (window.Player && !Player.dead) Player.damage(2);
          }
        }
      } else {
        // 归巢巡逻：锚定遗迹（30 格外折返）
        const hx = u.home.x - g.position.x, hz = u.home.z - g.position.z;
        if (hx * hx + hz * hz > 30 * 30) u.dir = Math.atan2(hz, hx);
        if (u.state === 'walk'){
          g.position.x += Math.cos(u.dir) * u.speed * 0.5 * dt;
          g.position.z += Math.sin(u.dir) * u.speed * 0.5 * dt;
          moving = true;
        }
      }
    } else {
      if (u.state === 'walk'){
        g.position.x += Math.cos(u.dir) * u.speed * 0.5 * dt;
        g.position.z += Math.sin(u.dir) * u.speed * 0.5 * dt;
        moving = true;
      }
    }
    // 悬浮高度：贴地形（+hoverAlt）+ 轻微浮动；追击时压低 1.5 格（未加载列保持当前高度）
    const gy = topAtRo(Math.floor(g.position.x), Math.floor(g.position.z));
    if (gy !== null){
      const alt = u.hoverAlt + Math.sin(u.animT * 1.3) * 0.45 + (u.aggro ? -1.5 : 0);
      const targetY = gy + 1 + alt;
      g.position.y += (targetY - g.position.y) * Math.min(1, dt * 3);
    }
    // 空中避障：机身高度有实心方块 → 快速爬升
    const by = Math.floor(g.position.y);
    for (let y = by; y <= by + 1; y++){
      const d = World.getDef(Math.floor(g.position.x), y, Math.floor(g.position.z));
      if (d && d.id !== 0 && !d.cross && !d.liquid && !d.lowbox){ g.position.y += 9 * dt; break; }
    }
    // 朝向 + 侧倾（转向时倾斜机身）
    const wantYaw = -u.dir - Math.PI / 2;
    let dy = wantYaw - g.rotation.y;
    dy = ((dy + Math.PI) % (Math.PI * 2) + Math.PI * 2) % (Math.PI * 2) - Math.PI;
    g.rotation.y += dy * Math.min(1, dt * 5);
    const turn = THREE.MathUtils.clamp(dy * 2, -0.5, 0.5);
    g.rotation.z += (turn - g.rotation.z) * Math.min(1, dt * 4);
    // 桨叶旋转（无人机） / 拍翼（浮翼）
    if (u.rotors) u.rotors.rotation.y += dt * (18 + (moving ? 12 : 0));
  }

  function tickOne(g, dt, plyPos){
    {
      const u = g.userData;
      // 联机：远端快照对齐（确定性模拟的漂移修正；快照过期后回到本地模拟）
      if (u.remote){
        const r = u.remote;
        r.t += dt;
        const k = Math.min(1, dt * 6);
        g.position.x += (r.x - g.position.x) * k;
        g.position.y += (r.y - g.position.y) * k;
        g.position.z += (r.z - g.position.z) * k;
        u.dir = r.dir;
        if (r.st) u.state = 'walk';
        if (r.t > 2.5) u.remote = null;
      }
      u.animT += dt;
      u.timer -= dt;
      // 受击闪红复原
      if (u.flashT > 0){
        u.flashT -= dt;
        if (u.flashT <= 0) clearHitFlash(g);
      }
      // 村民越界（离村心 > 10 格）→ 立即回家，不等待计时
      if (u.villager && u.state === 'idle'){
        const hx = u.home.x - g.position.x, hz = u.home.z - g.position.z;
        if (hx * hx + hz * hz > 10 * 10){
          u.state = 'walk';
          u.dir = Math.atan2(hz, hx);
          u.timer = 2 + u.rnd() * 3;
        }
      }
      if (u.timer <= 0){
        if (u.state === 'idle'){
          u.state = 'walk';
          if (u.villager){
            // 村民选向：向村心偏置（出圈一半概率朝家走），不再漫无目的乱跑
            const hx = u.home.x - g.position.x, hz = u.home.z - g.position.z;
            if (hx * hx + hz * hz > 6 * 6 && u.rnd() < 0.65){
              u.dir = Math.atan2(hz, hx) + (u.rnd() - 0.5) * 1.2;
            } else {
              u.dir = u.rnd() * Math.PI * 2;
            }
          } else {
            u.dir += (u.rnd() - 0.5) * 1.5;
          }
          u.timer = 2 + u.rnd() * 5;
        } else {
          u.state = 'idle';
          u.timer = 1.5 + u.rnd() * 3;
          if (u.baseSpeed !== undefined){ u.speed = u.baseSpeed; delete u.baseSpeed; }   // 逃窜结束恢复原速
        }
      }
      // 移动
      if (u.typeDef && u.typeDef.fly){
        flyMove(g, u, dt, plyPos);   // 飞行型：悬浮/追击，不走地面寻路与重力
      } else if (u.state === 'walk'){
        let nx = g.position.x + Math.cos(u.dir) * u.speed * dt;
        let nz = g.position.z + Math.sin(u.dir) * u.speed * dt;
        const curGy = topAtRo(Math.floor(g.position.x), Math.floor(g.position.z));
        if (curGy === null){
          // 所在区块未加载（理论上不常见）：本帧不移动，避免触发地形生成
          nx = g.position.x; nz = g.position.z;
        }
        if (u.villager){
          // 村民：漫游锚定村庄（离村心 10 格外折返）
          const hx = u.home.x - g.position.x, hz = u.home.z - g.position.z;
          if (hx * hx + hz * hz > 10 * 10) u.dir = Math.atan2(hz, hx);
        } else {
          // 野生生物：锚定出生领地（26 格外折返），不再远处转向玩家聚集
          const hx = u.home.x - g.position.x, hz = u.home.z - g.position.z;
          if (hx * hx + hz * hz > 26 * 26) u.dir = Math.atan2(hz, hx);
        }
        // 前方阻挡（树木/墙体/高台/未加载区块）→ 先尝试沿墙滑动，滑不动再转向（避免顶墙堆积）
        if (blockedAhead(u, nx, nz, curGy)){
          const sx = g.position.x + Math.cos(u.dir) * u.speed * dt;
          if (!blockedAhead(u, sx, g.position.z, curGy)){
            nx = sx; nz = g.position.z;
          } else {
            const sz = g.position.z + Math.sin(u.dir) * u.speed * dt;
            if (!blockedAhead(u, g.position.x, sz, curGy)){
              nx = g.position.x; nz = sz;
            } else {
              dodge(u);
              nx = g.position.x; nz = g.position.z;
            }
          }
        }
        // 贴地（仅落地时吸附；空中交给重力，避免跳跃/坠崖被逐帧拉回地面）
        if (u.onGround){
          const gy = topAtRo(Math.floor(nx), Math.floor(nz));
          if (gy === null){
            // 目标列未加载：不吸附不移动（下帧再试）
            nx = g.position.x; nz = g.position.z;
          } else {
            const targetY = gy + 1 + u.foot;
            if (targetY < g.position.y - 0.5){
              u.onGround = false;   // 前方悬空 → 转入自由落体
              g.position.set(nx, g.position.y, nz);
            } else {
              g.position.set(nx, THREE.MathUtils.lerp(g.position.y, targetY, dt * 6), nz);
            }
          }
        } else {
          g.position.x = nx; g.position.z = nz;
        }
        // 朝向：模型前方(-Z)对齐移动方向
        g.rotation.y = -u.dir - Math.PI / 2;
        // 跳跃：约 1 格高的小跳（能跃上一格台阶，远够不到 4 格以上的树冠），概率降低 + 冷却
        if (u.jumpCd > 0) u.jumpCd -= dt;
        if (u.typeDef.jump && u.onGround && u.jumpCd <= 0 && u.rnd() < 0.0004){
          u.jumpVel = 6.4;      // 跳高 ≈ 1.02 格（原 6 → 0.9；4.2 → 0.44 会连一格都上不去）
          u.onGround = false;
          u.jumpCd = 1.2;
        }
      }
      // 重力：空中自由落体 + 落地吸附（跳跃不再被当帧抵消）；飞行型无重力
      if (!u.typeDef || !u.typeDef.fly){
        if (!u.onGround){
          u.jumpVel -= 20 * dt;
          g.position.y += u.jumpVel * dt;
          const below = topAtRo(Math.floor(g.position.x), Math.floor(g.position.z));
          if (below !== null){   // 未加载列：维持下落，不触发生成
            const floorY = below + 1 + u.foot;
            if (g.position.y <= floorY && u.jumpVel <= 0){
              g.position.y = floorY;
              u.jumpVel = 0;
              u.onGround = true;
            }
          }
        }
      }
      if (u.isGlb && u.clips && u.clips.length){
        // ---- 精模：骨骼动画（Walk/Idle 交叉淡化，死亡单独处理）----
        if (!u.mixer){
          u.mixer = new THREE.AnimationMixer(g);
          const pick = preds => {
            for (const p of preds){
              const c = u.clips.find(x => p(x.name.toLowerCase()));
              if (c) return c;
            }
            return null;
          };
          u.clips.walk  = pick([n => n === 'walk' || n.endsWith('|walk'), n => n.includes('walk')]);
          u.clips.idle  = pick([n => n === 'idle' || n.endsWith('|idle'), n => n === 'idle_2' || n.endsWith('|idle_2'), n => n.includes('idle') && !n.includes('hit') && !n.includes('jump')]);
          u.clips.death = pick([n => n.includes('death')]);
          if (u.clips.walk && u.clips.idle){
            const aW = u.mixer.clipAction(u.clips.walk);
            const aI = u.mixer.clipAction(u.clips.idle);
            aW.setEffectiveWeight(0);
            aI.setEffectiveWeight(1);
            aW.play(); aI.play();
            u.anim = { walk: aW, idle: aI };
          }
        }
        if (u.anim && !u.dying){
          const wantWalk = u.state === 'walk' ? 1 : 0;
          const prev = u.anim.walk.getEffectiveWeight();
          const nw = prev + (wantWalk - prev) * Math.min(1, dt * 6);
          u.anim.walk.setEffectiveWeight(nw);
          u.anim.idle.setEffectiveWeight(1 - nw);
          u.anim.walk.timeScale = 0.75 + Math.min(1.6, u.speed * 0.7);
        }
        if (u.mixer) u.mixer.update(dt);
      } else if (u.villager && g.userData.rig){
        // ---- 村民：人形关节动画（四肢随行走摆动）----
        Humanoid.animate(g, dt, u.state === 'walk', u.speed);
      } else {
        // ---- 体素回退 ----
        if (u.rotors){
          // 无人机：桨叶在 flyMove 中旋转，机体轻微悬浮起伏
          if (g.children[0]) g.children[0].position.y = Math.sin(u.animT * 2) * 0.04;
        } else if (u.typeDef && u.typeDef.fly){
          // 浮翼/飞行体素生物：拍翼（children[1..2] 为侧翼）
          const flap = Math.sin(u.animT * (4 + u.speed * 2)) * 0.5;
          for (let i = 1; i <= 2 && i < g.children.length; i++){
            const w = g.children[i];
            if (w) w.rotation.z = flap * (w.position.x > 0 ? -1 : 1);
          }
          if (g.children[0]) g.children[0].position.y = Math.sin(u.animT * 2) * 0.05;
        } else {
          // 地面生物：腿绕髋关节摆动（children: 0=躯干, 1..4=腿关节）
          const legBob = u.state === 'walk' ? Math.sin(u.animT * (2 + u.speed * 3)) * 0.55 : 0;
          for (let i = 1; i <= 4 && i < g.children.length; i++){
            const leg = g.children[i];
            if (leg && leg.isGroup) leg.rotation.x = (i % 2 ? 1 : -1) * legBob;
          }
          // 行走时躯干起伏 / 待机呼吸
          if (g.children[0]){
            g.children[0].position.y = u.state === 'walk' ? Math.abs(Math.sin(u.animT * (2 + u.speed * 3))) * 0.04 : 0;
            g.children[0].scale.y = u.state === 'idle' ? 1 + Math.sin(u.animT * 2) * 0.03 : 1;
          }
        }
      }
    }
  }

  function reset(){
    if (group){
      for (const g of list) disposeObject3D(g, { skipGeo: !!g.userData.isGlb, skipTex: true });
      for (const g of dying) disposeObject3D(g, { skipGeo: !!g.userData.isGlb, skipTex: true });
      for (const f of fadingOut) disposeObject3D(f.g, { skipGeo: !!f.g.userData.isGlb, skipTex: true });
      for (const g of skyFlock) disposeObject3D(g, { skipGeo: true, skipTex: true, skipMat: true });
      group.clear();
    }
    list = [];
    dying.length = 0;
    fadingOut.length = 0;
    skyFlock.length = 0;
    ruinGuards.clear();
    cellStates.clear();
    herds.clear();
    removedMasks.clear();
    registerQueue.length = 0;
    lastCenter = null;
    lastInfoType = null;
    spawnTimer = 0;
    if (vGroup){
      for (const g of villagers) disposeObject3D(g, { skipGeo: true, skipTex: true, skipMat: true });   // 村民克隆共享模板几何/材质
      vGroup.clear();
    }
    villagers = [];
    spawnedVillages.clear();
    for (const [, gh] of ghosts){
      disposeObject3D(gh.g, { skipGeo: !!gh.g.userData.isGlb, skipTex: true, skipMat: !!gh.g.userData.isGlb });
    }
    ghosts.clear();
  }

  // ---------- 激光武器交互：射线命中 / 受击逃窜 / 死亡掉落 ----------
  const _rv = new THREE.Vector3();
  function rayHit(origin, dir, maxDist){
    let best = null, bestT = maxDist;
    for (const g of list){
      _rv.copy(g.position).sub(origin);
      _rv.y += (g.userData.radius || 0.8) * 0.4;   // 命中判定球心从脚底抬到躯干中部（修正：此前 -= 把球心压到脚底以下，瞄身体反而偏）
      const t = _rv.dot(dir);
      if (t < 0.6 || t > bestT) continue;
      const r = g.userData.radius || 0.8;
      if (_rv.lengthSq() - t * t < r * r){ best = g; bestT = t; }
    }
    return best ? { g: best, dist: bestT } : null;
  }
  function damage(g, dmg, fromPos, opts){
    const u = g.userData;
    if (u.hp === undefined) u.hp = 4;
    u.hp -= dmg;
    // 受击逃窜：背向攻击者加速跑（敌对守卫无人机不逃，继续战斗）
    if (!u.hostile){
      u.state = 'walk';
      u.timer = 2.5 + u.rnd() * 2;
      if (fromPos) u.dir = Math.atan2(g.position.z - fromPos.z, g.position.x - fromPos.x);
      if (u.baseSpeed === undefined) u.baseSpeed = u.speed;   // 记录原速，逃窜结束后恢复
      u.speed = Math.max(u.speed, (u.typeDef.speed || 1) * 2.4);
    }
    if (u.hp <= 0){ kill(g, opts); return true; }
    // 受击反馈：材质瞬时闪红（0.12s 后复原）+ 专属受击音
    flashHit(g, u);
    Sound.play('creatureHit');
    return false;
  }
  // 受击闪红：记录材质原 emissive，临时置红；tickOne 里到期复原
  function flashHit(g, u){
    u.flashT = 0.12;
    g.traverse(o => {
      const m = o.material;
      if (m && m.emissive && m._baseEm === undefined){
        m._baseEm = m.emissive.getHex();
        m.emissive.setHex(0xff2211);
      }
    });
  }
  function clearHitFlash(g){
    g.traverse(o => {
      const m = o.material;
      if (m && m.emissive && m._baseEm !== undefined){
        m.emissive.setHex(m._baseEm);
        delete m._baseEm;
      }
    });
  }
  function kill(g, opts){
    removeFromList(g);
    const u = g.userData;
    // 兽群记录立即删除（死亡动画只是视觉收尾；动画期间存档也记作已击杀，不会复活）
    removeHerdObject(g);
    // 遗迹守卫：营地死亡计数（玩家停留区域内不复活，离开再回来重新部署）
    if (u.sentinel && u.sentinelKey){
      const rec = ruinGuards.get(u.sentinelKey);
      if (rec){ rec.dead++; rec.alive = rec.alive.filter(x => x !== g); }
    }
    // 有死亡动画的精模：原地播放倒地动画后移除
    if (u.mixer && u.clips && u.clips.death){
      u.dying = { t: u.clips.death.duration + 0.1 };
      if (u.anim){ u.anim.walk.stop(); u.anim.idle.stop(); }
      const act = u.mixer.clipAction(u.clips.death);
      act.reset();
      act.setLoop(THREE.LoopOnce, 1);
      act.clampWhenFinished = true;
      act.play();
      dying.push(g);
    } else {
      disposeObject3D(g, { skipGeo: !!u.isGlb, skipTex: true });
      group.remove(g);
    }
    if (window.Player && (!opts || !opts.noDrop)){
      Player.spawnParticles(g.position.x, g.position.y + 0.3, g.position.z, 0xd4544a, 14);
      // 掉落表：按类型定义（守卫无人机掉电路板/装甲板，其余生物掉碳）
      const drops = (u.typeDef && u.typeDef.drops) || [{ item: 'carbon', n: 1 + ((Math.random() * 2) | 0) }];
      for (const dr of drops){
        if (dr.chance !== undefined && Math.random() > dr.chance) continue;
        Player.spawnDrop(g.position.x, g.position.y + 0.6, g.position.z, dr.item, dr.n || 1);
      }
    }
    Sound.play('creatureDie');
  }

  // ---------- 联机：快照 / 远端对齐 / 命中与击杀广播 ----------
  function findLocal(nid){
    for (const g of list){ if (g.userData.nid === nid) return g; }
    for (const g of villagers){ if (g.userData.nid === nid) return g; }
    return null;
  }
  // 快照：[nid, x×10, y×10, z×10, dir×100, walk?, hp, 村民?, 种类(0普通/1村民/2守卫)]
  function snapshot(plyPos, maxD = 90){
    if (!group) return null;
    const out = [];
    const push = g => {
      const d = Math.hypot(g.position.x - plyPos.x, g.position.z - plyPos.z);
      if (d > maxD) return;
      out.push([
        g.userData.nid,
        Math.round(g.position.x * 10), Math.round(g.position.y * 10), Math.round(g.position.z * 10),
        Math.round(g.userData.dir * 100),
        g.userData.state === 'walk' ? 1 : 0,
        Math.round(g.userData.hp || 0),
        g.userData.villager ? 1 : 0,
        g.userData.sentinel ? 2 : 0,
      ]);
    };
    for (const g of list) push(g);
    for (const g of villagers) push(g);
    return out.length ? out : null;
  }
  function ghostFor(entry){
    // entry 与 snapshot 同构
    const nid = entry[0];
    const isV = entry[7] === 1;
    const isS = entry[8] === 2;
    let g;
    if (isV){
      g = buildVillager(Math.abs(nid) % 5);
      const grig = rigFromClone(g);
      g.userData = { ghost: true, isGlb: true, rig: grig, nid };
      vGroup.add(g);
    } else if (isS){
      g = buildSentinel();
      const rotors = g.userData.rotors;
      g.userData = { ghost: true, nid, isGlb: false, typeDef: CREATURE_TYPES.sentinel, rotors, sentinel: true };
      group.add(g);
    } else {
      const info = (World.biome && World.biome.animal) || { type: 'strider', body: 0x888888, legs: 0x666666, eye: 0xffffff };
      const typeDef = CREATURE_TYPES[info.type] || CREATURE_TYPES.strider;
      g = buildCreature(typeDef, { body: info.body, legs: info.legs, eye: info.eye }, info.type);
      const foot = footOffset(g, typeDef);
      g.userData = { ghost: true, nid, foot, isGlb: !!g.userData.isGlb, typeDef };
      group.add(g);
    }
    return g;
  }
  function applyRemote(arr){
    if (!group || !Array.isArray(arr) || arr.length > 512) return;   // 多批次共存后 90m 内生物可超 256，放宽上限
    const now = performance.now();
    for (const e of arr){
      if (!Array.isArray(e) || e.length < 7 || !Number.isFinite(e[0]) || !e.slice(1, 5).every(Number.isFinite)) continue;
      const nid = e[0];
      const tgt = { x: e[1] / 10, y: e[2] / 10, z: e[3] / 10, dir: e[4] / 100, st: e[5] === 1, hp: e[6] | 0, villager: e[7] === 1 };
      const g = findLocal(nid);
      if (g){
        const u = g.userData;
        if (!u.villager && tgt.hp !== u.hp && tgt.hp >= 0) u.hp = tgt.hp;   // 血量对齐
        u.remote = { ...tgt, t: 0 };
      } else {
        let gh = ghosts.get(nid);
        if (!gh){
          const g2 = ghostFor(e);
          g2.position.set(tgt.x, tgt.y, tgt.z);
          g2.scale.setScalar(0.01);            // 淡入：不在玩家眼前凭空出现
          gh = { g: g2, tgt, last: now, fade: FADE_IN_T };
          ghosts.set(nid, gh);
        }
        gh.tgt = tgt;
        gh.last = now;
      }
    }
  }
  function remoteHit(nid, dmg){
    const g = findLocal(nid);
    if (g && !g.userData.villager) damage(g, dmg, null, { noDrop: true, remote: true });
  }
  function remoteKill(nid){
    const g = findLocal(nid);
    if (g && !g.userData.villager){
      kill(g, { noDrop: true, remote: true });
      return true;
    }
    const herd = herds.get(nid);
    if (herd){
      // 休眠中的兽群被远程击杀 → 删除记录（重载时不再出现）
      removeHerdRecord(herd);
      return true;
    }
    const gh = ghosts.get(nid);
    if (gh){
      disposeObject3D(gh.g, { skipGeo: !!gh.g.userData.isGlb, skipTex: true, skipMat: !!gh.g.userData.isGlb });
      group.remove(gh.g);
      ghosts.delete(nid);
    }
    return false;
  }

  return { init, update, tick, reset, rayHit, damage, kill, nearestVillager,
    snapshot, applyRemote, remoteHit, remoteKill, serialize, restore,
    // 测试钩子：在指定坐标部署一台守卫无人机（返回 Group；不入兽群/营地体系）
    debugSpawnSentinel(x, z){
      const g = materializeSentinel(x, z, sentinelNid(x | 0, z | 0, 0), mulberry32(sentinelNid(x | 0, z | 0, 0)), 'debug');
      g.userData.sentinelKey = null;   // 不参与营地重生计数
      g.userData.spawnT = 0;          // 免淡入
      g.scale.setScalar(1);
      return g;
    },
    debugSkyFlock(){ return skyFlock; },
    debugList(){ return list; },
    debugVillagers(){ return villagers; },
    debugCap(){ return CRE_CAP; },
    debugHerds(){ return herds.size; },
  };
})();
window.Creatures = Creatures;

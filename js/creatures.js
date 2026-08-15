/* ============================================================
   STARFORGE - creatures.js
   体素生物 + 精模生物：生成 / 漫游 / 跟随地形 / 跳跃
   · 精模（GLB）播放骨骼动画（Walk/Idle/Death），四肢随行走摆动
   · 体素回退模型腿关节在髋部旋转摆动
   · AI：锚定领地（野生生物守巢、村民守村）、矮跳不爬树、
     前方障碍转向（不再穿树穿墙）、不朝玩家聚集
   ============================================================ */
'use strict';

const Creatures = (() => {
  let group = null, list = [];
  let vGroup = null, villagers = [];        // 村民（独立分组：不参与刷新清理，不可被攻击）
  const dying = [];                         // 播放死亡动画中的精模生物
  const spawnedVillages = new Set();
  let batchCell = null;                     // 当前生物批次所在网格（跨玩家确定性生成）

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
    batchCell = null;
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

  // 地形判断：地面列顶是否是树木/液体（生物不生成在树上/水里）
  function solidDefAt(x, y, z){ return World.getDef(Math.floor(x), Math.floor(y), Math.floor(z)); }

  // plyPos: Vector3, biome: 星球生态对象
  function update(dt, plyPos, biome){
    if (!group) return;
    spawnVillages(plyPos);   // 靠近村庄时生成村民（每村一次）
    const info = biome.animal;
    if (!info) return;

    // 批次按 24m 网格：跨网格才重生成。同网格的玩家（联机）生成同一批生物
    const cx = Math.floor(plyPos.x / CRE_CELL), cz = Math.floor(plyPos.z / CRE_CELL);
    if (batchCell === cx + ',' + cz) return;
    batchCell = cx + ',' + cz;

    // 清理旧批（GLB 克隆几何共享模板跳过；体素回退全量）。投影生物（ghosts）保留
    for (const g of list) disposeObject3D(g, { skipGeo: !!g.userData.isGlb, skipTex: true });
    for (const g of dying) disposeObject3D(g, { skipGeo: !!g.userData.isGlb, skipTex: true });
    for (const g of list) group.remove(g);
    for (const g of dying) group.remove(g);
    dying.length = 0;
    list = [];

    const typeDef = CREATURE_TYPES[info.type] || CREATURE_TYPES.strider;
    const rnd = mulberry32(batchSeedOf(cx, cz));
    const ccx = cx * CRE_CELL + CRE_CELL / 2, ccz = cz * CRE_CELL + CRE_CELL / 2;
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
      const g = buildCreature(typeDef, { body: info.body, legs: info.legs, eye: info.eye }, info.type);
      // 贴地偏移：模型原点(躯干中心)到最低点（腿底）的距离，站在方块顶面(gy+1)上
      const foot = footOffset(g, typeDef);
      g.position.set(wx, gy + 1 + foot, wz);
      const glbClips = g.userData.clips || null;
      const nid = creatureNid(cx, cz, i);
      g.userData = {
        nid, rnd: mulberry32(nid),            // 联机：确定性行为随机源（跨客户端一致）
        speed: typeDef.speed * (0.5 + rnd()),
        dir: rnd() * Math.PI * 2,
        state: 'idle', timer: 1 + rnd() * 3,
        jumpVel: 0, onGround: true, jumpCd: 0,
        typeDef, animT: rnd() * 10, foot,
        hp: 4, isGlb: !!g.userData.isGlb,
        clips: glbClips,
        home: { x: wx, z: wz },             // 领地锚点：野生生物守巢，不再向玩家聚集
        radius: Math.max(0.55, Math.max(typeDef.w, typeDef.h, typeDef.d) * 1.3),
      };
      group.add(g);
      list.push(g);
    }
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
    for (const g of list) tickOne(g, dt, plyPos);
    for (const g of villagers) tickOne(g, dt, plyPos);
    // 死亡动画播完 → 移除
    for (let i = dying.length - 1; i >= 0; i--){
      const g = dying[i];
      const u = g.userData;
      u.dying.t -= dt;
      if (u.mixer) u.mixer.update(dt);
      if (u.dying.t <= 0){
        disposeObject3D(g, { skipGeo: !!u.isGlb, skipTex: true });
        group.remove(g);
        dying.splice(i, 1);
      }
    }
    // 联机投影生物：向快照位置平滑移动，4 秒无更新则移除
    const now = performance.now();
    for (const [nid, gh] of ghosts){
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
    const newGy = World.topAt(fx, fz);
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
      if (u.state === 'walk'){
        let nx = g.position.x + Math.cos(u.dir) * u.speed * dt;
        let nz = g.position.z + Math.sin(u.dir) * u.speed * dt;
        const curGy = World.topAt(Math.floor(g.position.x), Math.floor(g.position.z));
        if (u.villager){
          // 村民：漫游锚定村庄（离村心 10 格外折返）
          const hx = u.home.x - g.position.x, hz = u.home.z - g.position.z;
          if (hx * hx + hz * hz > 10 * 10) u.dir = Math.atan2(hz, hx);
        } else {
          // 野生生物：锚定出生领地（26 格外折返），不再远处转向玩家聚集
          const hx = u.home.x - g.position.x, hz = u.home.z - g.position.z;
          if (hx * hx + hz * hz > 26 * 26) u.dir = Math.atan2(hz, hx);
        }
        // 前方阻挡（树木/墙体/高台）→ 先尝试沿墙滑动，滑不动再转向（避免顶墙堆积）
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
          const gy = World.topAt(Math.floor(nx), Math.floor(nz));
          const targetY = gy + 1 + u.foot;
          if (targetY < g.position.y - 0.5){
            u.onGround = false;   // 前方悬空 → 转入自由落体
            g.position.set(nx, g.position.y, nz);
          } else {
            g.position.set(nx, THREE.MathUtils.lerp(g.position.y, targetY, dt * 6), nz);
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
      // 重力：空中自由落体 + 落地吸附（跳跃不再被当帧抵消）
      if (!u.onGround){
        u.jumpVel -= 20 * dt;
        g.position.y += u.jumpVel * dt;
        const below = World.topAt(Math.floor(g.position.x), Math.floor(g.position.z));
        const floorY = below + 1 + u.foot;
        if (g.position.y <= floorY && u.jumpVel <= 0){
          g.position.y = floorY;
          u.jumpVel = 0;
          u.onGround = true;
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
        // ---- 体素回退：腿绕髋关节摆动（children: 0=躯干, 1..4=腿关节）----
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

  function reset(){
    if (group){
      for (const g of list) disposeObject3D(g, { skipGeo: !!g.userData.isGlb, skipTex: true });
      for (const g of dying) disposeObject3D(g, { skipGeo: !!g.userData.isGlb, skipTex: true });
      group.clear();
    }
    list = [];
    dying.length = 0;
    if (vGroup){
      for (const g of villagers) disposeObject3D(g, { skipGeo: true, skipTex: true, skipMat: true });   // 村民克隆共享模板几何/材质
      vGroup.clear();
    }
    villagers = [];
    spawnedVillages.clear();
    batchCell = null;
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
      _rv.y -= (g.userData.radius || 0.8) * 0.4;   // 命中判定中心略高于脚底
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
    // 受击逃窜：背向攻击者加速跑
    u.state = 'walk';
    u.timer = 2.5 + u.rnd() * 2;
    if (fromPos) u.dir = Math.atan2(g.position.z - fromPos.z, g.position.x - fromPos.x);
    if (u.baseSpeed === undefined) u.baseSpeed = u.speed;   // 记录原速，逃窜结束后恢复
    u.speed = Math.max(u.speed, (u.typeDef.speed || 1) * 2.4);
    if (u.hp <= 0){ kill(g, opts); return true; }
    return false;
  }
  function kill(g, opts){
    const i = list.indexOf(g);
    if (i >= 0) list.splice(i, 1);
    const u = g.userData;
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
      Player.spawnDrop(g.position.x, g.position.y + 0.6, g.position.z, 'carbon', 1 + (Math.random() * 2 | 0));
    }
    Sound.play('breakBlk', 0.55);
  }

  // ---------- 联机：快照 / 远端对齐 / 命中与击杀广播 ----------
  function findLocal(nid){
    for (const g of list){ if (g.userData.nid === nid) return g; }
    for (const g of villagers){ if (g.userData.nid === nid) return g; }
    return null;
  }
  // 快照：[nid, x×10, y×10, z×10, dir×100, walk?, hp, 村民?]
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
    let g;
    if (isV){
      g = buildVillager(Math.abs(nid) % 5);
      const grig = rigFromClone(g);
      g.userData = { ghost: true, isGlb: true, rig: grig, nid };
      vGroup.add(g);
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
    if (!group || !Array.isArray(arr) || arr.length > 256) return;
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
          gh = { g: g2, tgt, last: now };
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
    const gh = ghosts.get(nid);
    if (gh){
      disposeObject3D(gh.g, { skipGeo: !!gh.g.userData.isGlb, skipTex: true, skipMat: !!gh.g.userData.isGlb });
      group.remove(gh.g);
      ghosts.delete(nid);
    }
    return false;
  }

  return { init, update, tick, reset, rayHit, damage, kill, nearestVillager,
    snapshot, applyRemote, remoteHit, remoteKill,
    debugList(){ return list; },
    debugVillagers(){ return villagers; },
  };
})();
window.Creatures = Creatures;

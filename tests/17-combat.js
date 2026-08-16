/* STARFORGE 测试套件 17 — 战斗：遗迹守卫无人机（追击 / 接触伤害 / 激光击杀与掉落） */
__SF_TEST__.suite('combat', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  // 同步步进生物逻辑（主循环也会调用 tick，但测试批内为原子任务，取样点与批边界一致）
  function step(n) {
    for (var i = 0; i < n; i++) window.Creatures.tick(1 / 30, window.Player.pos);
  }

  t.test('守卫无人机追击玩家', function () {
    var p = api.pos();
    var s = window.Creatures.debugSpawnSentinel(p[0] + 12, p[2] + 6);
    var d0 = Math.hypot(s.position.x - p[0], s.position.z - p[2]);
    A.ok(d0 > 8, 'spawned at distance (d0=' + d0.toFixed(1) + ')');
    step(60);   // 2s @30fps
    var d1 = Math.hypot(s.position.x - p[0], s.position.z - p[2]);
    A.ok(d1 < d0 - 2, 'sentinel closed in (d0=' + d0.toFixed(1) + ' → d1=' + d1.toFixed(1) + ')');
    window.Creatures.kill(s);
  });

  t.test('守卫无人机接触攻击造成伤害', function () {
    var p = api.pos();
    var s = window.Creatures.debugSpawnSentinel(p[0] + 1, p[2]);
    api.setStat('shield', 0);
    api.setStat('hp', 40);
    step(150);   // 5s：多次撞击（每次 2 点，冷却 1.15s）
    var hp = api.stats().hp;
    A.ok(hp < 40, 'player damaged by sentinel (hp=' + hp + ')');
    window.Creatures.kill(s);
  });

  t.test('激光击杀守卫并掉落电路板', function () {
    var p = api.pos();
    var s = window.Creatures.debugSpawnSentinel(p[0] + 3, p[2]);
    // 从玩家眼睛高度向守卫射线
    var origin = new THREE.Vector3(p[0], p[1] + 1.6, p[2]);
    var dir = new THREE.Vector3().subVectors(s.position, origin).normalize();
    var hit = window.Creatures.rayHit(origin, dir, 22);
    A.ok(hit && hit.g === s, 'raycast hits sentinel');
    var dropsBefore = window.Player.dropCount;
    window.Creatures.damage(s, 999);
    A.eq(window.Creatures.debugList().indexOf(s), -1, 'sentinel removed from list');
    A.ok(window.Player.dropCount > dropsBefore, 'loot dropped (before=' + dropsBefore + ', after=' + window.Player.dropCount + ')');
  });

  t.test('生物受击闪红并自动复原', function () {
    var p = api.pos();
    var s = window.Creatures.debugSpawnSentinel(p[0] + 3, p[2]);
    window.Creatures.damage(s, 1);   // 守卫 hp=10，存活 → 触发受击闪红
    var red = false;
    s.traverse(function (o){
      if (o.material && o.material.emissive && o.material.emissive.getHex() === 0xff2211) red = true;
    });
    A.ok(red, 'creature materials flash red on hit');
    step(10);   // 0.33s @30fps > flashT 0.12s
    var red2 = false;
    s.traverse(function (o){
      if (o.material && o.material.emissive && o.material.emissive.getHex() === 0xff2211) red2 = true;
    });
    A.ok(!red2, 'hit flash restored after 0.12s');
    window.Creatures.kill(s);
  });

  t.test('守卫攻击有前摇：先蓄力红眼再命中', function () {
    var p = api.pos();
    var s = window.Creatures.debugSpawnSentinel(p[0] + 1, p[2]);
    api.setStat('shield', 0);
    api.setStat('hp', 40);
    var hp0 = api.stats().hp;
    var sawWindup = false;
    // 第一段：只步进到「观察到前摇」或「掉血」为止——前摇必须先于掉血出现
    for (var i = 0; i < 60 && api.stats().hp === hp0; i++){
      window.Creatures.tick(1 / 30, window.Player.pos);
      if (s.userData.windup > 0) sawWindup = true;
    }
    A.ok(sawWindup, 'windup (eye charge) observed before any damage');
    // 第二段：前摇结束 → 命中
    for (var j = 0; j < 60 && api.stats().hp === hp0; j++){
      window.Creatures.tick(1 / 30, window.Player.pos);
    }
    A.ok(api.stats().hp < hp0, 'damage landed after windup (hp=' + api.stats().hp + ')');
    window.Creatures.kill(s);
  });

  t.test('生物 AI 地形查询不触发区块生成（topAtNoGen）', function () {
    var p = api.pos();
    // 部署在未加载区域（> 生成半径）：AI 悬浮查询绝不触发生成
    var s = window.Creatures.debugSpawnSentinel(p[0] + 120, p[2]);
    var g0 = window.World.genCount;
    step(120);   // 4s @30fps（距离门控下仍按 4 帧一次降频 tick）
    A.eq(window.World.genCount, g0, 'creature AI ticks generate zero chunks (gen=' + g0 + ')');
    window.Creatures.kill(s);
  });

  t.test('被水淹没的细胞不产生幽灵兽群（无效候选不注册）', function () {
    // 挑一个远离出生活动区的细胞，把候选环（细胞中心 12~92m）整圈淹水，
    // 使 8 次地形验证全部落在水面上 → 修复前这些候选以 (0,0) 初始值入列，
    // 掷骰命中即产生永久占据名额的幽灵兽群（水域生态大量出现、抑制真实生成）
    var p = api.pos();
    // 复制 creatures.js 的批次种子与随机消耗序：淹水后每次候选固定消耗
    // 8 次验证 × 2 抽 + 4 行为参数 = 20 抽；roll 是第 nCand×20+1 抽。
    // 选一个 roll < HERD_CHANCE(0.18) 的细胞，让「修复前必产生幽灵」可确定复现
    function batchSeedOf(cx, cz){
      var h = (window.World.seed ^ 0xC7EA5) >>> 0;
      h = Math.imul(h ^ cx, 374761393);
      h = Math.imul(h ^ cz, 668265263);
      h = (h ^ (h >>> 13)) >>> 0;
      return h;
    }
    var nCand = Math.min(window.World.biome.animal.count, 22);
    function rollOf(cx, cz){
      var rnd = api.mulberry32(batchSeedOf(cx, cz));
      for (var i = 0; i < nCand * 20; i++) rnd();
      return rnd();
    }
    var cx = Math.floor((p[0] + 300) / 24), cz = Math.floor(p[2] / 24);
    var guard = 0;
    while (rollOf(cx, cz) >= 0.18 && guard < 30){ cx++; guard++; }   // 找到掷骰命中的细胞
    var ccx = cx * 24 + 12, ccz = cz * 24 + 12;
    var waterId = api.defs.BLOCKS.water.id;
    var r = 104;
    for (var x = Math.floor(ccx - r); x <= ccx + r; x++){
      for (var z = Math.floor(ccz - r); z <= ccz + r; z++){
        var gy = window.World.topAt(x, z);
        window.World.set(x, gy, z, waterId, true);   // silent：跳过网格重建，只改地形数据
      }
    }
    var h0 = window.Creatures.debugHerds();
    window.Creatures.debugRegisterCell(cx, cz);
    var h1 = window.Creatures.debugHerds();
    A.eq(h1, h0, 'flooded cell registers zero phantom herds (herds ' + h0 + ' → ' + h1 + ')');
    // 无任何兽群停留在无效出生点 (0,0)（合法候选距细胞中心至少 12m，不可能落在原点）
    var s = window.Creatures.serialize();
    var phantoms = s.herds.filter(function (h) { return h[3] === 0 && h[4] === 0; });
    A.eq(phantoms.length, 0, 'no herd at invalid (0,0) spawn point');
    // 复位：重建干净世界（本测试淹水的区域不污染后续用例/套件）
    return api.reboot('normal');
  });

  t.test('密度统计用兽群活体位置（陈旧记录坐标不参与）', async function () {
    // 上一个用例以 reboot 收尾：先步进几帧让生态类型/注册状态在新世界上落定，
    // 否则本用例的 restore 可能撞上 update 里的 clearBatches（lastInfoType 未就绪）
    for (var s = 0; s < 3; s++) window.Creatures.update(1 / 30, window.Player.pos, window.World.biome);
    var alive = window.Creatures.debugList().slice();
    for (var q = 0; q < alive.length; q++) window.Creatures.kill(alive[q]);
    var sp = window.World.findSpawn();
    var x = 0, z = 0, ok = false;
    for (var r = 0; r < 64 && !ok; r++){
      for (var dx = -r; dx <= r && !ok; dx++){
        for (var dz = -r; dz <= r && !ok; dz++){
          if (Math.max(Math.abs(dx), Math.abs(dz)) !== r) continue;
          var gx = Math.floor(sp.x) + dx, gz = Math.floor(sp.z) + dz;
          var gy2 = window.World.topAt(gx, gz);
          var dd2 = window.World.getDef(gx, gy2, gz);
          if (dd2 && !dd2.liquid && dd2.key !== 'log' && dd2.key !== 'leaves'){ x = gx; z = gz; ok = true; }
        }
      }
    }
    A.ok(ok, 'found a valid standing spot near spawn');
    var cx = Math.floor(x / 24), cz = Math.floor(z / 24);
    // 目标兽群的 nid（candIdx=0）：精确匹配，避免同细胞自然生成的兽群干扰
    function batchSeedOf(cx2, cz2){
      var h = (window.World.seed ^ 0xC7EA5) >>> 0;
      h = Math.imul(h ^ cx2, 374761393);
      h = Math.imul(h ^ cz2, 668265263);
      h = (h ^ (h >>> 13)) >>> 0;
      return h;
    }
    var myNid = batchSeedOf(cx, cz) * 64 + 0;
    window.Creatures.restore({ herds: [[cx, cz, 0, (x + 0.5) * 10, (z + 0.5) * 10, 4, (x + 0.5) * 10, (z + 0.5) * 10]], removed: [] });
    var found = null;
    await api.waitUntil(function () {
      for (var i = 0; i < 3; i++) window.Creatures.update(1 / 30, window.Player.pos, window.World.biome);
      var list = window.Creatures.debugList();
      for (var j = 0; j < list.length; j++){
        var u = list[j].userData;
        if (u.herd && u.herd.nid === myNid){ found = list[j]; break; }
      }
      return !!found;
    }, 5000, 50);
    A.ok(found, 'herd materialized (spot=' + x + ',' + z + ' cell=' + cx + ',' + cz + ' herds=' + window.Creatures.debugHerds() + ' active=' + window.Creatures.debugList().length + ')');
    // 相对计数断言：同细胞可能还有自然兽群，挪走目标兽群后计数应恰好 -1。
    // 修复前统计用记录坐标（停在玩家脚下）→ 挪走活体后计数不变 → 断言失败
    var c0 = window.Creatures.debugLocalHerdCount(window.Player.pos);
    A.ok(c0 >= 1, 'herd counted local before move (c0=' + c0 + ')');
    found.position.x += 200;
    A.eq(window.Creatures.debugLocalHerdCount(window.Player.pos), c0 - 1, 'herd moved 200m away excluded from count');
    found.position.x -= 170;   // 挪回 30m 内
    A.eq(window.Creatures.debugLocalHerdCount(window.Player.pos), c0, 'herd back within 30m counted again');
  });

  t.test('物化扫描不漏掉远格但近距离的休眠兽群（快路径门限）', async function () {
    for (var s = 0; s < 3; s++) window.Creatures.update(1 / 30, window.Player.pos, window.World.biome);
    var alive = window.Creatures.debugList().slice();
    for (var q = 0; q < alive.length; q++) window.Creatures.kill(alive[q]);
    var sp = window.World.findSpawn();
    var x = 0, z = 0, ok = false;
    for (var r = 0; r < 64 && !ok; r++){
      for (var dx = -r; dx <= r && !ok; dx++){
        for (var dz = -r; dz <= r && !ok; dz++){
          if (Math.max(Math.abs(dx), Math.abs(dz)) !== r) continue;
          var gx = Math.floor(sp.x) + dx, gz = Math.floor(sp.z) + dz;
          var gy2 = window.World.topAt(gx, gz);
          var dd2 = window.World.getDef(gx, gy2, gz);
          if (dd2 && !dd2.liquid && dd2.key !== 'log' && dd2.key !== 'leaves'){ x = gx; z = gz; ok = true; }
        }
      }
    }
    A.ok(ok, 'found a valid standing spot near spawn');
    function batchSeedOf(cx2, cz2){
      var h = (window.World.seed ^ 0xC7EA5) >>> 0;
      h = Math.imul(h ^ cx2, 374761393);
      h = Math.imul(h ^ cz2, 668265263);
      h = (h ^ (h >>> 13)) >>> 0;
      return h;
    }
    // 记录细胞距玩家 7 格（旧快路径 6 格门限会漏），位置却在玩家近旁（<96m 物化半径）
    var pcx = Math.floor(window.Player.pos.x / 24), pcz = Math.floor(window.Player.pos.z / 24);
    var hcx = pcx + 7, hcz = pcz;
    var myNid = batchSeedOf(hcx, hcz) * 64 + 0;
    window.Creatures.restore({ herds: [[hcx, hcz, 0, (x + 0.5) * 10, (z + 0.5) * 10, 4, (x + 0.5) * 10, (z + 0.5) * 10]], removed: [] });
    var found = null;
    await api.waitUntil(function () {
      for (var i = 0; i < 3; i++) window.Creatures.update(1 / 30, window.Player.pos, window.World.biome);
      var list = window.Creatures.debugList();
      for (var j = 0; j < list.length; j++){
        var u = list[j].userData;
        if (u.herd && u.herd.nid === myNid){ found = list[j]; break; }
      }
      return !!found;
    }, 5000, 50);
    A.ok(found, 'dormant herd 7 cells away but within 96m materialized');
  });

  t.test('读档恢复兽群行为参数由 nid 确定性派生（联机两端一致）', function () {
    // 在出生点（地形必然有效）恢复一个兽群，步进物化后断言
    // speed/dir/timer/animT 全部等于 nid 派生值——修复前 animT 用 Math.random、
    // speed/dir 用 1/0 占位，各客户端相位漂移、行为不一致
    // 清空活跃生物（避免 CRE_CAP 卡住物化；不能用 reset()——它重置 lastInfoType，
    // 下一次 update 的 clearBatches 会把刚恢复的兽群清掉）
    var alive = window.Creatures.debugList().slice();
    for (var q = 0; q < alive.length; q++) window.Creatures.kill(alive[q]);
    // 出生点可能压在树冠下（topAt 顶块为 leaves → 物化校验拒绝），
    // 从出生点向外确定性搜索一块可站立的实心地面放置兽群
    var sp = window.World.findSpawn();
    var x = 0, z = 0, ok = false;
    for (var r = 0; r < 64 && !ok; r++){
      for (var dx = -r; dx <= r && !ok; dx++){
        for (var dz = -r; dz <= r && !ok; dz++){
          if (Math.max(Math.abs(dx), Math.abs(dz)) !== r) continue;
          var gx = Math.floor(sp.x) + dx, gz = Math.floor(sp.z) + dz;
          var gy2 = window.World.topAt(gx, gz);
          var dd2 = window.World.getDef(gx, gy2, gz);
          if (dd2 && !dd2.liquid && dd2.key !== 'log' && dd2.key !== 'leaves'){ x = gx; z = gz; ok = true; }
        }
      }
    }
    A.ok(ok, 'found a valid standing spot near spawn');
    var cx = Math.floor(x / 24), cz = Math.floor(z / 24);
    window.Creatures.restore({ herds: [[cx, cz, 0, (x + 0.5) * 10, (z + 0.5) * 10, 4, (x + 0.5) * 10, (z + 0.5) * 10]], removed: [] });
    var found = null;
    for (var i = 0; i < 10 && !found; i++){
      window.Creatures.update(1 / 30, window.Player.pos, window.World.biome);
      var list = window.Creatures.debugList();
      for (var j = 0; j < list.length; j++){
        var u = list[j].userData;
        if (u.herd && u.herd.cx === cx && u.herd.cz === cz){ found = u; break; }
      }
    }
    A.ok(found, 'restored herd materialized near player');
    // 对当前所有已物化兽群逐一断言「行为参数 = nid 派生值」：
    // 该不变式对恢复/新建/补全三条路径同时成立，且不依赖单个兽群的候选索引
    var td = api.defs.CREATURE_TYPES[window.World.biome.animal.type] || {};
    var checked = 0;
    for (var k = 0; k < window.Creatures.debugList().length; k++){
      var uu = window.Creatures.debugList()[k].userData;
      if (!uu.herd) continue;   // 守卫无人机等非兽群实体跳过
      checked++;
      var rnd2 = api.mulberry32(uu.nid);
      var expSpeed = 0.5 + rnd2() * 0.5, expDir = rnd2() * Math.PI * 2;
      var expTimer = 1 + rnd2() * 3, expAnimT = rnd2() * 10;
      A.ok(Math.abs(uu.speed - (td.speed || 1) * expSpeed) < 1e-9, 'speed deterministic (got ' + uu.speed + ', want ' + ((td.speed || 1) * expSpeed) + ')');
      A.ok(Math.abs(uu.dir - expDir) < 1e-9, 'dir deterministic (got ' + uu.dir + ', want ' + expDir + ')');
      A.ok(Math.abs(uu.timer - expTimer) < 1e-9, 'timer deterministic (got ' + uu.timer + ', want ' + expTimer + ')');
      A.ok(Math.abs(uu.animT - expAnimT) < 1e-9, 'animT deterministic (got ' + uu.animT + ', want ' + expAnimT + ')');
    }
    A.ok(checked > 0, 'at least one herd checked (' + checked + ')');
  });

  t.test('高空浮翼淡入出场（不从全尺寸凭空弹出）', function () {
    if (!(window.World.biome && window.World.biome.skywings)){
      A.eq(1, 1, 'biome has no skywings — skip');
      return;
    }
    var before = window.Creatures.debugSkyFlock().length;
    A.eq(window.Creatures.debugSpawnSkyFlock(window.Player.pos), true, 'flock spawned');
    var fl = window.Creatures.debugSkyFlock();
    A.ok(fl.length > before, 'new skywings added');
    for (var i = before; i < fl.length; i++){
      A.ok(fl[i].scale.x < 0.05, 'new skywing starts tiny for fade-in (scale=' + fl[i].scale.x.toFixed(3) + ')');
    }
    // 步进若干帧：scale 应逐渐增长（淡入进行中，而非瞬间全尺寸）
    // 增速 dt×3/帧 → 5 帧 ≈ 0.51，远未到 1
    for (var j = 0; j < 5; j++) window.Creatures.update(1 / 30, window.Player.pos, window.World.biome);
    var grown = fl[before];
    A.ok(grown && grown.scale.x > 0.05 && grown.scale.x < 1.0, 'skywing grows gradually (scale=' + (grown ? grown.scale.x.toFixed(3) : 'gone') + ')');
  });
});

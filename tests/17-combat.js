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

  t.test('生物 AI 地形查询不触发区块生成（topAtNoGen）', function () {
    var p = api.pos();
    // 部署在未加载区域（> 生成半径）：AI 悬浮查询绝不触发生成
    var s = window.Creatures.debugSpawnSentinel(p[0] + 120, p[2]);
    var g0 = window.World.genCount;
    step(120);   // 4s @30fps（距离门控下仍按 4 帧一次降频 tick）
    A.eq(window.World.genCount, g0, 'creature AI ticks generate zero chunks (gen=' + g0 + ')');
    window.Creatures.kill(s);
  });
});

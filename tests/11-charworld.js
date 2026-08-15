/* STARFORGE 测试套件 11 — 人物/世界分离存档 · 外观自定义 · 生物AI · 骨骼动画 */
__SF_TEST__.suite('charworld', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('new game creates char and world records', function () {
    return api.listChars().then(function (chars) {
      A.ok(chars.length >= 1, 'at least one character record');
      A.ok(!!chars[0].key, 'char has key');
      A.eq(chars[0].name, '测试旅行者', 'char name from creation flow');
      return api.listWorlds();
    }).then(function (worlds) {
      A.ok(worlds.length >= 1, 'at least one world record');
      A.ok(!!worlds[0].key, 'world has key');
      A.eq(worlds[0].name, '测试世界', 'world name from creation flow');
    });
  });

  t.test('character customization persists through save/load', function () {
    var app = { skin: '#8d5a3c', hairStyle: 'mohawk', hair: '#c23a3a', suit: '#3fa8c9',
      trim: '#ffd94d', pants: '#443430', boots: '#33261a', helmet: true, visor: '#b58aff' };
    window.Player.appearance = app;
    api.setCredits(321);
    return api.save('外观测试').then(function (ok) {
      A.ok(ok, 'save ok');
      window.Player.appearance = null;
      return api.listSaves();
    }).then(function (saves) {
      return api.load(saves[0].key);
    }).then(function (loaded) {
      A.ok(loaded, 'load ok');
      var a = window.Player.appearance;
      A.ok(!!a, 'appearance restored');
      A.eq(a.skin, '#8d5a3c', 'skin color restored');
      A.eq(a.hairStyle, 'mohawk', 'hair style restored');
      A.eq(a.hair, '#c23a3a', 'hair color restored');
      A.eq(a.suit, '#3fa8c9', 'suit color restored');
      A.eq(a.visor, '#b58aff', 'visor color restored');
      A.eq(a.helmet, true, 'helmet flag restored');
      A.eq(api.credits(), 321, 'credits restored with char');
    });
  });

  t.test('createCharacter / deleteChar management', function () {
    return api.createCharacter('测试小号', { skin: '#e8d0b0', hairStyle: 'none', suit: '#5a6a3a' }).then(function (key) {
      A.ok(!!key, 'char created');
      return api.listChars();
    }).then(function (chars) {
      var c = null;
      for (var i = 0; i < chars.length; i++) if (chars[i].name === '测试小号') c = chars[i];
      A.ok(!!c, 'new char listed');
      return api.deleteChar(c.key);
    }).then(function () {
      return api.listChars();
    }).then(function (chars) {
      A.ok(!chars.some(function (x) { return x.name === '测试小号'; }), 'char deleted');
    });
  });

  t.test('second character joins existing world via loadPair', function () {
    return api.createCharacter('二号旅行者', { skin: '#f0d8b8', hairStyle: 'pony', suit: '#5a3e3e' }).then(function (charKey) {
      return api.listWorlds().then(function (worlds) {
        api.setCredits(1);
        return api.loadPair(charKey, worlds[0].key);
      }).then(function (ok) {
        A.ok(ok, 'loadPair loaded');
        A.eq(api.state(), 'planet', 'in planet state');
        A.eq(api.credits(), 250, 'second char fresh credits (250)');
        A.ok(!!window.Player.appearance, 'second char appearance applied');
        A.eq(window.Player.appearance.hairStyle, 'pony', 'pony hairstyle applied');
      });
    });
  });

  t.test('villagers stay anchored to village home', function () {
    var st = null;
    var strs = window.World.structures || [];
    for (var i = 0; i < strs.length; i++) if (strs[i].type === 'village'){ st = strs[i]; break; }
    if (!st){ A.ok(true, 'no village on this planet — skipped'); return; }
    window.Player.pos.set(st.x, window.World.topAt(st.x, st.z) + 2, st.z);
    window.Creatures.update(0.016, window.Player.pos, window.World.biome);
    var vs = window.Creatures.debugVillagers();
    A.ok(vs.length >= 1, 'villagers spawned near village');
    var maxD = 0;
    for (var f = 0; f < 6000; f++){
      window.Creatures.tick(0.05, window.Player.pos);
      for (var v = 0; v < vs.length; v++){
        var d = Math.hypot(vs[v].position.x - vs[v].userData.home.x, vs[v].position.z - vs[v].userData.home.z);
        if (d > maxD) maxD = d;
      }
    }
    A.ok(maxD <= 10.8, 'villagers stayed near home over 300s sim (max ' + maxD.toFixed(2) + 'm)');
  });

  t.test('creature cannot climb onto a tree trunk', function () {
    window.Creatures.update(0.016, window.Player.pos, window.World.biome);
    var list = window.Creatures.debugList();
    A.ok(list.length > 0, 'creatures spawned');
    var g = list[0], u = g.userData;
    // 确定性场地：平整起点与柱位及柱两侧（跳跃 1 格高落在平地上，不会误判）
    var ax = Math.abs(Math.cos(u.dir)) > Math.abs(Math.sin(u.dir)) ? (Math.cos(u.dir) > 0 ? 1 : -1) : 0;
    var az = ax === 0 ? (Math.sin(u.dir) > 0 ? 1 : -1) : 0;
    var gx0 = Math.floor(g.position.x), gz0 = Math.floor(g.position.z);
    var base = window.World.topAt(gx0, gz0);
    g.position.set(gx0 + 0.5, base + 1 + u.foot, gz0 + 0.5);
    function flatten(cx, cz){
      var cgy = window.World.topAt(cx, cz);
      if (cgy < base){ for (var y = cgy + 1; y <= base; y++) window.World.set(cx, y, cz, BLOCKS.stone.id); }
      else if (cgy > base){ for (var y = base + 1; y <= cgy; y++) window.World.set(cx, y, cz, 0); }
      for (var y2 = base + 1; y2 <= base + 8; y2++) window.World.set(cx, y2, cz, 0);
    }
    var tx = gx0 + ax * 2, tz = gz0 + az * 2;
    flatten(gx0, gz0); flatten(gx0 + ax, gz0 + az);
    flatten(tx, tz); flatten(tx + az, tz + ax); flatten(tx - az, tz - ax);
    for (var y3 = 1; y3 <= 3; y3++) window.World.set(tx, base + y3, tz, BLOCKS.stone.id);   // 3 格树干
    u.state = 'walk'; u.timer = 999;
    u.speed = Math.max(u.speed, 1.5);
    var startY = g.position.y, maxY = startY;
    for (var i = 0; i < 600; i++){
      window.Creatures.tick(0.05, window.Player.pos);
      if (g.position.y > maxY) maxY = g.position.y;
      u.dir = Math.atan2(az, ax);
      u.state = 'walk'; u.timer = 999;
    }
    A.ok(maxY < startY + 1.4, 'creature never climbed the trunk (maxY ' + maxY.toFixed(2) + ')');
    for (var y4 = 1; y4 <= 3; y4++) window.World.set(tx, base + y4, tz, 0);
  });

  t.test('creature can step up a single block', function () {
    window.Creatures.update(0.016, window.Player.pos, window.World.biome);
    var list = window.Creatures.debugList();
    A.ok(list.length > 0, 'creatures spawned');
    var g = list[0], u = g.userData;
    // 确定性场地：沿生物朝向取主轴，平整一条 5 格走廊（清树/填坑/削高），终点放 1 格台阶
    var ax = Math.abs(Math.cos(u.dir)) > Math.abs(Math.sin(u.dir)) ? (Math.cos(u.dir) > 0 ? 1 : -1) : 0;
    var az = ax === 0 ? (Math.sin(u.dir) > 0 ? 1 : -1) : 0;
    var gx0 = Math.floor(g.position.x), gz0 = Math.floor(g.position.z);
    var base = window.World.topAt(gx0, gz0);
    g.position.set(gx0 + 0.5, base + 1 + u.foot, gz0 + 0.5);   // 起点贴地
    function flatten(cx, cz){
      var cgy = window.World.topAt(cx, cz);
      if (cgy < base){ for (var y = cgy + 1; y <= base; y++) window.World.set(cx, y, cz, BLOCKS.stone.id); }
      else if (cgy > base){ for (var y = base + 1; y <= cgy; y++) window.World.set(cx, y, cz, 0); }
      for (var y2 = base + 1; y2 <= base + 8; y2++) window.World.set(cx, y2, cz, 0);
    }
    var tx = gx0 + ax * 5, tz = gz0 + az * 5;
    for (var k = 1; k <= 4; k++) flatten(gx0 + ax * k, gz0 + az * k);
    flatten(tx, tz);
    window.World.set(tx, base + 1, tz, BLOCKS.stone.id);   // 一格台阶
    u.state = 'walk'; u.timer = 999;
    u.speed = Math.max(u.speed, 1.5);
    var climbed = false;
    for (var i = 0; i < 400; i++){
      window.Creatures.tick(0.05, window.Player.pos);
      u.dir = Math.atan2(az, ax);   // 沿走廊直行
      u.state = 'walk'; u.timer = 999;
      var groundNow = window.World.topAt(Math.floor(g.position.x), Math.floor(g.position.z));
      if (groundNow >= base + 1 && Math.abs(g.position.x - (tx + 0.5)) < 1.2 && Math.abs(g.position.z - (tz + 0.5)) < 1.2) climbed = true;
    }
    A.ok(climbed, 'creature stepped onto the single block');
    window.World.set(tx, base + 1, tz, 0);
  });

  t.test('rigged clones preserve skeleton bone order', function () {
    if (!window.ModelLib || !window.ModelLib.getTemplate){ A.ok(true, 'ModelLib missing — skipped'); return; }
    var tpl = window.ModelLib.getTemplate('strider');
    if (!tpl){ A.ok(true, 'strider template not ready — skipped'); return; }
    var tplSk = null;
    tpl.scene.traverse(function (o){ if (o.isSkinnedMesh && !tplSk) tplSk = o; });
    var clone = window.ModelLib.get('strider', 2, {});
    var sk = null;
    clone.traverse(function (o){ if (o.isSkinnedMesh && !sk) sk = o; });
    A.ok(!!tplSk && !!sk, 'template & clone both have skinned meshes');
    var tplNames = tplSk.skeleton.bones.map(function (b){ return b.name; });
    var clNames = sk.skeleton.bones.map(function (b){ return b.name; });
    A.eq(clNames.length, sk.skeleton.boneInverses.length, 'clone bones == boneInverses count');
    A.eq(JSON.stringify(clNames), JSON.stringify(tplNames), 'clone skeleton matches template bone order');
    window.disposeObject3D(clone, { skipGeo: true, skipTex: true, skipMat: true });
  });

  t.test('glb creatures run skeletal walk/idle animation', function () {
    window.Creatures.update(0.016, window.Player.pos, window.World.biome);
    var list = window.Creatures.debugList();
    A.ok(list.length > 0, 'creatures spawned');
    for (var i = 0; i < 90; i++) window.Creatures.tick(0.05, window.Player.pos);
    var rigged = 0;
    for (var j = 0; j < list.length; j++){
      var u = list[j].userData;
      if (u.mixer && u.clips && u.clips.walk && u.clips.idle) rigged++;
    }
    A.ok(rigged > 0, 'skeletal walk/idle clips active on ' + rigged + ' creatures');
  });

  t.test('crossing a 24m batch cell keeps nearby creatures alive', function () {
    // 等全部 3×3 批次补齐（每帧最多建 2 个批次）
    for (var f = 0; f < 30; f++){
      window.Creatures.update(0.016, window.Player.pos, window.World.biome);
      window.Creatures.tick(0.016, window.Player.pos);
    }
    var before = window.Creatures.debugList();
    A.ok(before.length > 0, 'creatures spawned');
    var nearIds = [];
    for (var i = 0; i < before.length; i++){
      var g = before[i];
      if (Math.hypot(g.position.x - window.Player.pos.x, g.position.z - window.Player.pos.z) < 60) nearIds.push(g.userData.nid);
    }
    A.ok(nearIds.length > 0, 'some creatures near player before crossing (' + before.length + ' total)');
    // 玩家跨过 24m 批次边界：近处生物不能被整体清空（旧逻辑会在这里全部销毁）
    window.Player.pos.x += 24;
    for (var f2 = 0; f2 < 30; f2++){
      window.Creatures.update(0.016, window.Player.pos, window.World.biome);
      window.Creatures.tick(0.016, window.Player.pos);
    }
    var after = window.Creatures.debugList();
    var ids = {};
    for (var j = 0; j < after.length; j++) ids[after[j].userData.nid] = true;
    var kept = 0;
    for (var k = 0; k < nearIds.length; k++) if (ids[nearIds[k]]) kept++;
    A.eq(kept, nearIds.length, 'all nearby creatures survived the cell crossing (' + kept + '/' + nearIds.length + ')');
    window.Player.pos.x -= 24;   // 复位，避免影响其他用例
  });

  t.test('creatures never materialize right in front of the player', function () {
    // 瞬移到全新区域：新批次里出生点离玩家近的一律暂缓生成（距离门控 + 淡入）
    window.Player.pos.x += 600;
    window.Creatures.update(0.016, window.Player.pos, window.World.biome);
    var list = window.Creatures.debugList();
    var tooClose = 0;
    for (var i = 0; i < list.length; i++){
      var g = list[i];
      if (Math.hypot(g.position.x - window.Player.pos.x, g.position.z - window.Player.pos.z) < 33) tooClose++;
    }
    A.eq(tooClose, 0, 'no creature materialized within 33m of the player (' + list.length + ' total)');
    window.Player.pos.x -= 600;
  });
});

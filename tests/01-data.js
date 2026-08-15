/* STARFORGE 测试套件 01 — 静态数据完整性（无需启动星球） */
__SF_TEST__.suite('data', function (t, api) {
  var A = api.assert;

  t.test('items self-consistent', function () {
    var I = api.defs.ITEMS;
    for (var id in I) {
      A.eq(I[id].id, id, 'item id ' + id);
      A.ok(I[id].name, 'item name ' + id);
      A.ok(I[id].stack > 0, 'item stack ' + id);
    }
  });

  t.test('blocks self-consistent and roundtrip', function () {
    var B = api.defs.BLOCKS, BID = api.defs.BLOCK_BY_ID;
    for (var k in B) {
      A.eq(B[k].key, k, 'block key ' + k);
      A.ok(B[k].name, 'block name ' + k);
      A.eq(BID[B[k].id].key, k, 'BLOCK_BY_ID roundtrip ' + k);
    }
  });

  t.test('item.block references valid blocks', function () {
    var I = api.defs.ITEMS, B = api.defs.BLOCKS;
    for (var id in I) if (I[id].block) A.ok(B[I[id].block], 'block ' + id + ' -> ' + I[id].block);
  });

  t.test('recipes valid and indexed', function () {
    var R = api.defs.RECIPES, RBID = api.defs.RECIPE_BY_ID, I = api.defs.ITEMS;
    A.ok(R.length > 0, 'recipes exist');
    for (var i = 0; i < R.length; i++) {
      var r = R[i];
      A.ok(r.id && r.time > 0, 'recipe ' + r.id + ' time');
      A.ok(['hand', 'both', 'furnace', 'assembler', 'refinery'].indexOf(r.where) >= 0, 'recipe where ' + r.id);
      for (var k in r.in) A.ok(I[k], 'recipe in ' + r.id + ' ' + k);
      for (var k2 in r.out) A.ok(I[k2], 'recipe out ' + r.id + ' ' + k2);
      A.eq(RBID[r.id], r, 'recipe by id ' + r.id);
    }
  });

  t.test('tech valid', function () {
    var T = api.defs.TECH, I = api.defs.ITEMS;
    for (var id in T) {
      var x = T[id];
      A.eq(x.id, id, 'tech id ' + id);
      A.ok(Array.isArray(x.req), 'tech req array ' + id);
      for (var i = 0; i < x.req.length; i++) A.ok(T[x.req[i]], 'tech req ' + id + ' -> ' + x.req[i]);
      for (var c in x.cost) A.ok(I[c], 'tech cost item ' + id + ' ' + c);
    }
    A.ok(T.survival && T.survival.unlocked, 'survival unlocked default');
  });

  t.test('quests valid', function () {
    var Q = api.defs.QUESTS, B = api.defs.BLOCKS, T = api.defs.TECH, I = api.defs.ITEMS;
    A.ok(Q.length > 0, 'quests exist');
    for (var i = 0; i < Q.length; i++) {
      var q = Q[i];
      A.ok(q.id && q.title && q.type, 'quest ' + q.id);
      A.ok(['collect', 'place', 'tech', 'event'].indexOf(q.type) >= 0, 'quest type ' + q.id);
      if (q.type === 'collect') A.ok(I[q.item] && q.n > 0, 'collect ' + q.id);
      if (q.type === 'place') A.ok(B[q.block], 'place ' + q.id);
      if (q.type === 'tech') A.ok(T[q.tech], 'tech ' + q.id);
      if (q.type === 'event') A.ok(q.flag, 'event flag ' + q.id);
    }
  });

  t.test('planets/biomes/trade valid', function () {
    var P = api.defs.SYSTEM_PLANETS, B = api.defs.BIOMES, I = api.defs.ITEMS;
    A.ok(P.length >= 5, 'home system has planets');
    for (var i = 0; i < P.length; i++) A.ok(B[P[i].biome], 'biome ' + P[i].biome);
    var tg = api.defs.TRADE_GOODS;
    for (var j = 0; j < tg.length; j++) A.ok(I[tg[j]], 'trade good ' + tg[j]);
    var bp = api.defs.STATION_BLUEPRINTS;
    for (var k = 0; k < bp.length; k++) A.ok(api.defs.TECH[bp[k].tech], 'blueprint tech ' + bp[k].tech);
  });

  t.test('galaxy generation deterministic', function () {
    var a = api.generateGalaxy(777), b = api.generateGalaxy(777);
    A.eq(a.name, b.name, 'galaxy name deterministic');
    A.eq(a.planets, b.planets, 'galaxy planet count deterministic');
    A.between(a.planets, 4, 7, 'galaxy planet count 4..7');
    A.ok(a.station && a.station.length === 3, 'station 3d');
    A.ok(a.market && Object.keys(a.market).length > 0, 'market populated');
  });

  t.test('像素图集使用最近 mip 采样（近距硬边 / 远距消闪）', function () {
    A.eq(Tex.texture.magFilter, THREE.NearestFilter, 'atlas magFilter nearest');
    A.eq(Tex.texture.minFilter, THREE.NearestMipmapNearestFilter, 'atlas minFilter nearest-mipmap');
    A.eq(Tex.texture.generateMipmaps, true, 'atlas mipmaps enabled');
    var t = Tex.tileTexture('stone');
    A.eq(t.magFilter, THREE.NearestFilter, 'tile magFilter nearest');
    A.eq(t.minFilter, THREE.NearestMipmapNearestFilter, 'tile minFilter nearest-mipmap');
  });
});

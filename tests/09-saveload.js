/* STARFORGE 测试套件 09 — 存档/读档（IndexedDB 多槽位往返） */
__SF_TEST__.suite('saveload', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('save -> list -> load roundtrip', function () {
    api.clearInv(); api.setCredits(500);
    api.give('carbon', 42);
    return api.save('自动化测试档').then(function (ok) {
      A.ok(ok, 'save succeeded');
      return api.listSaves();
    }).then(function (saves) {
      A.ok(saves.length >= 1, 'listed at least one');
      var key = saves[0].key;
      // 污染当前状态后读档
      api.clearInv(); api.setCredits(1);
      return api.load(key).then(function (loaded) {
        A.ok(loaded, 'load succeeded');
        A.eq(api.credits(), 500, 'credits restored');
        A.eq(api.count('carbon'), 42, 'inventory restored');
        A.eq(api.state(), 'planet', 'back in planet state');
      });
    });
  });

  t.test('saveTo overwrites an existing slot', function () {
    return api.listSaves().then(function (saves) {
      var key = saves[0] && saves[0].key;
      if (!key) return;
      api.setCredits(777);
      return api.saveTo(key, '覆盖档').then(function () {
        return api.load(key);
      }).then(function () {
        A.eq(api.credits(), 777, 'overwritten credits');
      });
    });
  });

  t.test('deleteSave removes a slot', function () {
    return api.save('待删除').then(function () {
      return api.listSaves();
    }).then(function (after) {
      var target = after[0].key;
      return api.deleteSave(target).then(function () {
        return api.listSaves();
      }).then(function (fin) {
        A.ok(fin.every(function (s) { return s.key !== target; }), 'slot deleted');
      });
    });
  });

  t.test('完整区块快照：读档后区块来自落盘数据（Minecraft 式持久化）', function () {
    // 生成并修改出生点外的区块 (3,3)，然后存档
    var x = 60, z = 60, y = Math.min(api.topAt(x, z) + 1, 95);
    api.setBlock(x, y, z, 'stone');
    A.eq(api.blockKeyAt(x, y, z), 'stone', '修改生效');
    var pcx = Math.floor(api.pos()[0] / 16), pcz = Math.floor(api.pos()[2] / 16);
    return api.save('快照测试档').then(function (ok) {
      A.ok(ok, 'save succeeded');
      return api.listSaves();
    }).then(function (saves) {
      A.ok(saves.length >= 1, '有可用档案');
      var key = saves[0].key;
      return api.load(key).then(function (loaded) {
        A.ok(loaded, 'load succeeded');
        A.eq(api.blockKeyAt(x, y, z), 'stone', '修改过的方块从区块快照还原');
        A.ok(api.chunkSaved(x, z), '修改区块来自完整快照（非程序化重生成）');
        A.ok(api.chunkSaved(pcx * 16, pcz * 16), '出生点区块同样来自完整快照');
        return api.deleteSave(key);
      });
    });
  });

  t.test('loadPair 新建配对条目字段完整（列表不显示 undefined）', async function () {
    var s = await api.save('配对源档');
    A.ok(s, 'save ok');
    var saves = await api.listSaves();
    var src = saves.find(function (e) { return e.name === '配对源档'; });
    A.ok(src && src.charKey && src.worldKey, 'entry has char+world keys');
    await api.deleteSave(src.key);   // 只删配对条目，保留人物与世界
    await api.loadPair(src.charKey, src.worldKey);
    var after = await api.listSaves();
    var ne = after.find(function (e) { return e.charKey === src.charKey && e.worldKey === src.worldKey; });
    A.ok(ne, 'new pair entry created by loadPair');
    A.ok(Number.isFinite(ne.credits), 'credits numeric (got ' + JSON.stringify(ne.credits) + ')');
    A.ok(Number.isFinite(ne.playMin), 'playMin numeric (got ' + JSON.stringify(ne.playMin) + ')');
    A.ok(typeof ne.charName === 'string' && ne.charName.length > 0, 'charName present');
    A.ok(typeof ne.worldName === 'string' && ne.worldName.length > 0, 'worldName present');
    A.ok(typeof ne.planetName === 'string' && ne.planetName.length > 0, 'planetName present');
    await api.deleteSave(ne.key);   // 清理
  });
});

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
});

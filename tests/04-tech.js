/* STARFORGE 测试套件 04 — 科技树（研究/前置/花费/计时研究） */
__SF_TEST__.suite('tech', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('survival unlocked by default', function () {
    A.ok(api.tech('survival'), 'survival unlocked');
    A.ok(api.techList().indexOf('survival') >= 0, 'survival in techList');
  });

  t.test('research requires cost', function () {
    A.eq(api.canResearch('metallurgy'), false, 'no data yet');
    A.eq(api.research('metallurgy'), false, 'research fails without cost');
    A.ok(!api.tech('metallurgy'), 'still locked');
  });

  t.test('research requires prerequisites', function () {
    api.give('data', 99); api.give('circuit', 99); api.give('uranium', 99);
    A.eq(api.research('power'), false, 'power needs automation prereq');
    A.ok(!api.tech('power'), 'power still locked');
  });

  t.test('research chain with costs and prereqs', function () {
    api.give('data', 99); api.give('circuit', 99); api.give('titanium', 99);
    api.give('uranium', 99); api.give('gold', 99); api.give('tritium', 99);
    A.ok(api.research('metallurgy'), 'metallurgy');
    A.ok(api.tech('metallurgy'));
    A.ok(api.research('automation'), 'automation');
    A.ok(api.research('logistics'), 'logistics');
    A.ok(api.research('power'), 'power (after automation)');
    A.ok(api.tech('power'));
  });

  t.test('timed research completes', function () {
    api.give('data', 99);
    return api.researchTimed('scan1').then(function (done) {
      A.ok(done, 'scan1 timed research completes');
      A.ok(api.tech('scan1'), 'scan1 unlocked');
    });
  });
});

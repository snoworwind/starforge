/* STARFORGE 测试套件 16 — UI 细节修复（捏人高亮 / 预览裁切 / 输入法回车） */
__SF_TEST__.suite('uipolish', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true }); });

  t.test('捏人色板：点击高亮且随机外观后每组恰好一个高亮', function () {
    window.UI.openCharCreate(false, 4);
    var chips = document.querySelectorAll('#ccSkin .cc-chip');
    A.ok(chips.length >= 2, 'skin chips built');
    chips[2].onclick();
    var on = document.querySelectorAll('#ccSkin .cc-chip.on');
    A.eq(on.length, 1, 'exactly one chip highlighted');
    A.eq(on[0].dataset.hex, chips[2].dataset.hex, 'highlight is the clicked chip');
    // 随机外观 → refreshCharSwatches()：修复前 style.background===hex 恒 false 会全灭
    document.getElementById('btnCharRandom').onclick();
    var groups = ['ccSkin', 'ccHair', 'ccSuit', 'ccTrim', 'ccPants', 'ccBoots', 'ccVisor'];
    var okAll = groups.every(function (id) {
      return document.querySelectorAll('#' + id + ' .cc-chip.on').length === 1;
    });
    A.ok(okAll, 'every color group has exactly one highlight after random');
    document.getElementById('btnCharBack').onclick();
  });

  t.test('捏人 3D 预览：HiDPI 下画布按 CSS 尺寸显示（无裁切）', function () {
    window.UI.openCharCreate(false, 4);
    var cv = document.getElementById('charPrevCanvas');
    A.eq(cv.style.width, '300px', 'preview canvas CSS width set (no HiDPI crop), got=' + cv.style.width);
    A.eq(cv.style.height, '360px', 'preview canvas CSS height set, got=' + cv.style.height);
    document.getElementById('btnCharBack').onclick();
  });

  t.test('聊天输入：IME 候选确认不误发，普通回车发送', async function () {
    await Net.joinRoom('127.0.0.1:17886');
    A.ok(Net.active(), 'connected to test server');
    Net.openChat();
    var inp = document.getElementById('chatInput');
    A.ok(inp, 'chat input exists');
    inp.value = '你好';
    var ime = new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', isComposing: true, bubbles: true, cancelable: true });
    inp.dispatchEvent(ime);
    A.eq(inp.value, '你好', 'IME confirm does not send or clear');
    var nor = new KeyboardEvent('keydown', { key: 'Enter', code: 'Enter', bubbles: true, cancelable: true });
    inp.dispatchEvent(nor);
    A.eq(inp.value, '', 'normal Enter clears input (send)');
    Net.disconnect();
  });

  t.test('音量滑杆持久化：改动写回 localStorage 并恢复显示', function () {
    var slider = document.getElementById('volSlider');
    A.ok(slider, 'volume slider exists');
    A.eq(slider.value, String(Math.round(Sound.getVolume() * 100)), 'slider shows persisted volume (default 70)');
    slider.value = '35';
    slider.oninput({ target: slider });
    A.eq(JSON.parse(localStorage.getItem('starforge_volume')), 0.35, 'volume persisted to localStorage');
    A.eq(Math.round(Sound.getVolume() * 100), 35, 'Sound.getVolume updated');
    // 还原默认，避免影响后续
    slider.value = '70';
    slider.oninput({ target: slider });
  });

  t.test('入口版本自检：基准跟随 __V_MAIN，不误报「文件版本不齐」', function () {
    var badge = document.getElementById('verBadge');
    A.ok(badge, 'version badge element exists');
    var main = window.__V_MAIN || '';
    A.ok(main, '__V_MAIN defined');
    var ok = !/文件版本不齐/.test(badge.textContent);
    A.ok(ok, 'badge reports ok, got: ' + badge.textContent);
    A.ok(badge.textContent.indexOf(main) >= 0, 'badge mentions current module version');
    A.eq(window.__V_STATION, main, 'station version in sync');
    A.eq(window.__V_SPACE, main, 'space version in sync');
  });

  t.test('机器面板按状态签名节流重建（不打断交互）', function () {
    var y = api.topAt(60, 60) + 1;
    api.placeMachine('chest', 60, y, 60, 0);
    window.UI.openMachinePanel(window.Factory.at(60, y, 60));
    var grid = document.querySelector('#machineBody .slot-grid');
    A.ok(grid, 'chest grid built');
    var firstSlot = grid.children[0];
    var i;
    for (i = 0; i < 10; i++) window.UI.tickMachinePanel(0.5);   // 10 个节流窗口
    var grid2 = document.querySelector('#machineBody .slot-grid');
    A.ok(grid2 === grid, 'DOM not rebuilt while chest static');
    A.ok(grid2.children[0] === firstSlot, 'slot element identity preserved (drag/click 不被打断)');
    // 物品变化 → 触发重建
    api.machineInsert(60, y, 60, 'stone');
    window.UI.tickMachinePanel(0.5);
    var grid3 = document.querySelector('#machineBody .slot-grid');
    A.ok(grid3 !== grid, 'DOM rebuilt after inventory change');
    window.UI.closeAll();
    api.removeMachine(60, y, 60);
  });
});

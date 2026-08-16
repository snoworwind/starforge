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

  t.test('聊天框默认隐藏 + 玩家面板纳入 anyPanelOpen/closeAll', async function () {
    await Net.joinRoom('127.0.0.1:17886');
    A.ok(Net.active(), 'connected to test server');
    Net.ensureChatUI();
    Net.ensurePlayersUI();
    var inp = document.getElementById('chatInput');
    A.ok(inp, 'chat input created');
    A.ok(inp.classList.contains('hidden'), 'chat input hidden until opened (fix: 不再凭空出现)');
    Net.openChat();
    A.ok(!inp.classList.contains('hidden'), 'openChat reveals input');
    Net.closeChat();
    A.ok(inp.classList.contains('hidden'), 'closeChat hides input again');
    Net.togglePlayers();
    var pp = document.getElementById('playersPanel');
    A.ok(pp && !pp.classList.contains('hidden'), 'players panel open');
    A.ok(window.UI.anyPanelOpen(), 'anyPanelOpen sees players panel');
    window.UI.closeAll();
    A.ok(pp.classList.contains('hidden'), 'closeAll hides players panel');
    Net.disconnect();
  });

  t.test('玩家面板开启前 closeAll（不与其他面板堆叠）', async function () {
    await Net.joinRoom('127.0.0.1:17886');
    document.getElementById('pausePanel').classList.remove('hidden');
    Net.togglePlayers();
    var pp = document.getElementById('playersPanel');
    A.ok(pp && !pp.classList.contains('hidden'), 'players panel open');
    A.ok(document.getElementById('pausePanel').classList.contains('hidden'), 'other panels closed by togglePlayers');
    window.UI.closeAll();
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

  t.test('单机系统面板打开 → 世界真正冻结，关闭后恢复', async function () {
    A.eq(window.Game.worldPaused(), false, 'not paused at baseline');
    // 打开暂停菜单，轮询等待主循环感知（固定短 sleep 在高负载下偶发抖动）
    document.getElementById('pausePanel').classList.remove('hidden');
    await api.waitUntil(function () { return window.Game.worldPaused(); }, 5000, 50);
    A.ok(window.Game.worldPaused(), 'worldPaused() true while pause menu open');
    var t0 = window.Game.playTime;
    await api.sleep(350);
    A.eq(window.Game.playTime, t0, 'playTime frozen while paused (got +' + (window.Game.playTime - t0).toFixed(3) + 's)');
    document.getElementById('pausePanel').classList.add('hidden');
    A.eq(window.Game.worldPaused(), false, 'worldPaused() false after closing');
    await api.waitUntil(function () { return window.Game.playTime > t0; }, 5000, 50);
    A.ok(window.Game.playTime > t0, 'world resumes after closing pause menu');
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

  t.test('世界创建页难度按钮生效且高亮回显上一屏选择', async function () {
    // 真实新档流程：diffSelect 选困难 → 捏人 → 世界页应回显困难；
    // 页内改选简单 → 最终进入的难度必须是简单（修复前页内按钮只改高亮、实际仍按困难进入）
    document.getElementById('btnDiffHard').onclick();
    A.eq(document.getElementById('charCreate').classList.contains('hidden'), false, 'char create shown');
    document.getElementById('charNameInput').value = '难度测试员';
    document.getElementById('btnCharConfirm').onclick();
    A.ok(document.getElementById('btnWcHard').classList.contains('on'), 'world panel echoes hard from diffSelect');
    document.getElementById('btnWcEasy').onclick();
    A.ok(document.getElementById('btnWcEasy').classList.contains('on'), 'easy button highlights');
    document.getElementById('worldNameInput').value = '难度测试世界';
    document.getElementById('btnWorldConfirm').onclick();
    await api.waitUntil(function () { return window.Game.state === 'planet'; }, 90000, 30);
    A.eq(window.Game.creative, false, 'not creative');
    A.eq(window.Game.dropMult, 7, 'world panel easy choice honored (dropMult=7)');
    await api.reboot('normal');   // 复位干净世界
  });

  t.test('背包满时 Shift 取回不静默丢失（溢出掉落在身旁）', function () {
    api.clearInv();
    var i;
    for (i = 0; i < 36; i++) api.give('stone', 250);   // 36 格 × 250 = 塞满
    A.eq(api.count('stone'), 9000, 'inventory full');
    var y = api.topAt(60, 60) + 1;
    api.placeMachine('chest', 60, y, 60, 0);
    api.machineInsert(60, y, 60, 'iron');
    window.UI.openMachinePanel(window.Factory.at(60, y, 60));
    var drops0 = window.Player.dropCount;
    var slot = document.querySelector('#machineBody .slot-grid .slot');
    A.ok(slot, 'chest slot rendered');
    slot.onmousedown({ shiftKey: true, button: 0, preventDefault: function () {} });
    A.ok(window.Player.dropCount > drops0, 'overflow dropped instead of lost (drops ' + drops0 + '→' + window.Player.dropCount + ')');
    A.eq(api.count('iron'), 0, 'iron did not fit into full inventory');
    window.UI.closeAll();
    api.removeMachine(60, y, 60);
  });

  t.test('切窗失焦清除全部按键（keyup 被吞不残留）', function () {
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyW', bubbles: true, cancelable: true }));
    A.eq(window.Player.keys.KeyW, true, 'W held after keydown');
    window.dispatchEvent(new Event('blur'));
    A.eq(window.Player.keys.KeyW, false, 'W released on blur');
    var allClear = true;
    for (var k in window.Player.keys){ if (window.Player.keys[k]) allClear = false; }
    A.ok(allClear, 'all player keys cleared on blur');
  });

  t.test('地图标记标签按纯文本渲染（防 HTML 注入）', function () {
    var pid = window.Game.currentPlanet;
    var saved = window.Game.mapMarks[pid] || [];
    window.Game.mapMarks[pid] = [{ x: 1, z: 2, y: 3, label: '<b>炸</b>', gal: false }];
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyM', bubbles: true, cancelable: true }));   // 打开星球地图
    var nm = document.querySelector('#mapMarkList .mm-name');
    A.ok(nm, 'mark row rendered');
    A.eq(nm.children.length, 0, 'no element injected into label');
    A.eq(nm.textContent, '⚑ <b>炸</b>', 'label shown as plain text');
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'KeyM', bubbles: true, cancelable: true }));   // 关闭地图
    window.Game.mapMarks[pid] = saved;   // 还原世界状态
  });

  // 回归：openSavePanel 手写隐藏列表漏掉星系图/地图/联机/玩家列表——太空里打开星系图再开
  // 存档面板，星图叠在存档面板后面。修复：改用 closeAll 与所有面板互斥
  t.test('save panel closes all other panels (incl galaxy map)', async function () {
    await window.Game.tpTo(0, null, 'space', 'test');
    window.Space.shipState.pos.set(5000, 5000, 5000);
    window.UI.openGalaxyMap();
    A.ok(!document.getElementById('galaxyPanel').classList.contains('hidden'), 'galaxy map open');
    await window.UI.openSavePanel('save');
    A.ok(document.getElementById('galaxyPanel').classList.contains('hidden'), 'galaxy map closed by save panel');
    window.UI.closeAll();
  });
});

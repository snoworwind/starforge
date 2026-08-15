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
});

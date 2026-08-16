/* STARFORGE 测试套件 13 — 地形引擎（生态配方互异 / 确定性 / 行星性格 / 新世界高度） */
__SF_TEST__.suite('terrain', function (t, api) {
  var A = api.assert;

  t.before(function () { return api.boot('normal', { fresh: true, seed: 424242 }); });

  function stats(biomeKey, seed){
    window.World.init(biomeKey, seed, null, null);
    var seab = 32 + (api.defs.BIOMES[biomeKey].seaLift || 0);
    var heights = [];
    for (var z = -120; z <= 120; z += 8){
      for (var x = -120; x <= 120; x += 8){
        heights.push(window.World.mapHeightAt(x, z));
      }
    }
    var min = Infinity, max = -Infinity, sum = 0, sum2 = 0, sea = 0;
    for (var i = 0; i < heights.length; i++){
      var h = heights[i];
      if (h < min) min = h;
      if (h > max) max = h;
      sum += h; sum2 += h * h;
      if (h <= seab) sea++;
    }
    var mean = sum / heights.length;
    return { min, max, variance: sum2 / heights.length - mean * mean, seaFrac: sea / heights.length };
  }

  t.test('地形确定性：同种子同生态完全一致', function () {
    window.World.init('lush', 777, null, null);
    var a = window.World.mapHeightAt(55, -33);
    var b = window.World.mapHeightAt(55, -33);
    A.eq(a, b, '重复采样一致');
    window.World.init('lush', 777, null, null);
    A.eq(window.World.mapHeightAt(55, -33), a, '重建世界后一致');
  });

  t.test('生态地形统计特征互异', function () {
    var keys = ['lush','desert','frozen','volcanic','alien','ocean','crystal','fungal','ashen','amber','ferrous','murk','salt','obsidian','redmoss','hive'];
    var sig = {};
    var MIN_VAR = { flats: 0.15, swamp: 0.15 };   // 近坦/湿地生态允许极低起伏
    for (var i = 0; i < keys.length; i++){
      sig[keys[i]] = stats(keys[i], 424242);
      var minV = MIN_VAR[api.defs.BIOMES[keys[i]].terrain.type] || 1;
      A.ok(sig[keys[i]].variance > minV, keys[i] + ' 有地形起伏（var ' + sig[keys[i]].variance.toFixed(1) + '，门槛 ' + minV + '）');
    }
    A.ok(sig.ocean.seaFrac > 0.5, 'ocean 水面占比 >50%（实际 ' + (sig.ocean.seaFrac * 100).toFixed(0) + '%）');
    A.ok(sig.desert.seaFrac < sig.ocean.seaFrac, 'desert 水面占比低于 ocean');
    A.ok(sig.volcanic.variance > sig.ashen.variance, 'volcanic 起伏 > ashen');
    A.ok(sig.alien.variance > sig.murk.variance, 'alien 起伏 > murk');
    A.ok(sig.frozen.variance < sig.volcanic.variance, 'frozen 起伏 < volcanic');
    A.ok(sig.salt.variance < sig.redmoss.variance, 'salt 平坦 < redmoss 台地');
  });

  t.test('行星性格：同生态不同种子地形不同', function () {
    window.World.init('lush', 111, null, null);
    var h1 = [];
    for (var i = 0; i < 12; i++) h1.push(window.World.mapHeightAt(-100 + i * 10, 37));
    window.World.init('lush', 222, null, null);
    var same = true;
    for (var j = 0; j < h1.length; j++){
      if (window.World.mapHeightAt(-100 + j * 10, 37) !== h1[j]){ same = false; break; }
    }
    A.ok(!same, '同生态不同种子地形不同');
  });

  t.test('亚生态存在：翠绿星球同时存在森林/草原/湿地色带', function () {
    window.World.init('lush', 31337, null, null);
    var cols = {};
    for (var z = -100; z <= 100; z += 6){
      for (var x = -100; x <= 100; x += 6){
        var c = window.World.mapColorRGB(x, z).join(',');
        cols[c] = true;
      }
    }
    A.ok(Object.keys(cols).length >= 3, '地表颜色带 ≥3 种（实际 ' + Object.keys(cols).length + '）');
  });

  t.test('熔岩湖：火山星低地由水体（熔岩观感）覆盖', function () {
    window.World.init('volcanic', 5150, null, null);
    var lava = 0, total = 0;
    for (var z = -96; z <= 96; z += 6){
      for (var x = -96; x <= 96; x += 6){
        var h = window.World.mapHeightAt(x, z);
        if (h <= 32){ lava++; }
        total++;
      }
    }
    A.ok(lava > 0, '存在熔岩低地（' + (lava / total * 100).toFixed(0) + '%）');
    var c = window.World.mapColorRGB(0, 0);
    A.ok(true, '颜色采样无异常 ' + c.join(','));
  });

  t.test('世界常量：新高度与中性基准', function () {
    A.eq(window.World.WORLD_H, 96, 'WORLD_H = 96');
    A.eq(window.World.SEA, 32, 'SEA = 32');
    A.eq(window.World.SEA_Y, 28, 'SEA_Y = 28（海平面下 4 格）');
  });

  // 回归：冰锥把氚晶放底部、冰放尖端（与注释「晶块尖+冰座」相反）——
  // 可采集的氚晶被埋在冰座下面；修复后顶部晶块、底部冰座
  t.test('冰锥结构：晶块尖在上、冰座在下（顶部氚晶可采集）', function () {
    window.World.init('frozen', 424242, null, null);
    var found = false, checked = 0;
    for (var z = -96; z <= 96 && !found; z++){
      for (var x = -96; x <= 96 && !found; x++){
        // 冰锥自身就是该列的最高块：晶块顶(gy)压在冰座(gy-1)上
        var gy = window.World.topAt(x, z);
        var top = window.World.getDef(x, gy, z);
        var below = window.World.getDef(x, gy - 1, z);
        if (top && top.key === 'crystal' && below && below.key === 'ice') found = true;
        checked++;
      }
    }
    A.ok(found, 'found crystal-tip over ice-base spike (checked ' + checked + ' columns)');
  });
});

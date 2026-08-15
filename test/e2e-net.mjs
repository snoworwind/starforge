// STARFORGE 联机端到端验证（真实双浏览器：房主 + 访客）
// 用法：npm run test:e2e（自动拉起默认端口 17888/17889 的服务器，结束自动关闭）
// 覆盖：房间创建（中途上传世界）→ 访客自动进入同一世界 → 化身可见 →
//       聊天 → 方块改动同步 → 一键传送 → 昼夜同步 → 世界持久化到磁盘
import { chromium } from 'playwright-core';
import { spawn } from 'node:child_process';
import { rm, readFile, access } from 'node:fs/promises';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const SAVE = join(ROOT, '.e2e-save');
await rm(SAVE, { recursive: true, force: true });
const srv = spawn(process.execPath, [join(ROOT, 'server.mjs'), '--save-dir', SAVE, '--reset'], { stdio: 'inherit' });
for (let i = 0; i < 40; i++){
  try { const r = await fetch('http://127.0.0.1:17888/__status'); if (r.ok) break; } catch(e){}
  await new Promise(r => setTimeout(r, 250));
}

const browser = await chromium.launch({
  channel: 'msedge', headless: true,
  args: ['--enable-unsafe-swiftshader', '--use-gl=angle', '--use-angle=swiftshader', '--disable-dev-shm-usage'],
});
async function newPage(){
  const ctx = await browser.newContext({ viewport: { width: 800, height: 600 } });
  await ctx.addInitScript(() => {
    try { localStorage.setItem('starforge_settings', JSON.stringify({ fov: 75, chunkDist: 6, farDist: 400, quality: 'low', planetLod: 'low', clouds: 'off', realAtmo: 'off', npcShips: 0 })); } catch(e){}
  });
  const p = await ctx.newPage();
  p.on('pageerror', e => console.log(`[pageerror] ${e && e.message}`));
  p.on('crash', () => console.log('[page crash]'));
  await p.goto('http://127.0.0.1:17888/index.html?test=1', { waitUntil: 'domcontentloaded', timeout: 60000 });
  await p.waitForFunction(() => window.__SF_TEST__ && window.__SF_TEST__.ready, null, { timeout: 120000 });
  return p;
}

const results = [];
const ok = (name, cond, extra) => { results.push({ name, pass: !!cond, extra }); console.log(`${cond ? 'PASS' : 'FAIL'} ${name}${extra ? ' · ' + extra : ''}`); };

try {
  // ---- 房主 A：开始游戏（模拟房主已有世界，中途创建房间）----
  const A = await newPage();
  await A.evaluate(async () => { await __SF_TEST__.boot('normal', { fresh: true }); });
  const seedA = await A.evaluate(() => window.World.seed);
  await A.evaluate(async () => { await Net.hostRoom('127.0.0.1'); });
  ok('房主中途创建房间', await A.evaluate(() => Net.active() && Net.role === 'host'));
  ok('房主世界已上传', await A.evaluate(() => Net.serverInfo && Net.serverInfo.hasWorld === true));

  // ---- 访客 B：从主菜单加入，自动进入同一世界 ----
  const B = await newPage();
  await B.evaluate(async () => { await Net.joinRoom('127.0.0.1'); });
  // 诊断：每 5 秒报告 B 的状态
  const diag = setInterval(async () => {
    try {
      const s = await B.evaluate(() => ({ st: window.Game ? Game.state : '?', net: Net.active(), gotInit: Net.gotInit }));
      console.log(`[B diag] state=${s.st} net=${s.net} gotInit=${s.gotInit}`);
      if (s.st === 'planet') clearInterval(diag);
    } catch(e){ console.log('[B diag] page unreachable:', e.message); }
  }, 5000);
  await B.waitForFunction(() => window.Game.state === 'planet', null, { timeout: 90000 });
  clearInterval(diag);
  ok('访客自动进入世界', true);
  ok('访客世界种子一致', await B.evaluate(s => window.World.seed === s, seedA));
  ok('访客角色独立（新手初始背包）', await B.evaluate(() => window.Player.inv.some(s => s && s.item === 'carbon')));

  // ---- 化身互见（位置同步）----
  await A.waitForFunction(() => window.Net.getRemotes().length >= 1, null, { timeout: 15000 });
  ok('房主看到访客化身', true);
  await B.waitForFunction(() => window.Net.getRemotes().length >= 1, null, { timeout: 15000 });
  ok('访客看到房主化身', true);
  const aName = await A.evaluate(() => Net.myName);
  const bName = await B.evaluate(() => Net.myName);
  ok('名字互不相同（自动去重）', aName !== bName, `${aName} vs ${bName}`);

  // ---- 聊天双向 ----
  await A.evaluate(() => Net.sendChat('房主向访客问好'));
  await B.waitForFunction(() => (document.getElementById('chatBox').textContent || '').includes('房主向访客问好'), null, { timeout: 8000 });
  ok('房主→访客聊天', true);
  await B.evaluate(() => Net.sendChat('访客收到'));
  await A.waitForFunction(() => (document.getElementById('chatBox').textContent || '').includes('访客收到'), null, { timeout: 8000 });
  ok('访客→房主聊天', true);

  // ---- 方块改动同步（访客放置 → 房主可见）----
  const spot = await B.evaluate(() => {
    const sp = window.World.findSpawn();
    return { x: Math.floor(sp.x) + 3, y: Math.floor(sp.y) + 1, z: Math.floor(sp.z) };
  });
  await B.evaluate(s => { window.World.set(s.x, s.y, s.z, 18 /* glass */); }, spot);
  await A.waitForFunction(s => window.World.get(s.x, s.y, s.z) === 18, spot, { timeout: 10000 });
  ok('访客放方块 → 房主世界同步', true);

  // ---- 一键传送：访客 → 房主 ----
  await A.evaluate(async () => { await __SF_TEST__.setPos(50, 40, 50); });
  await new Promise(r => setTimeout(r, 500));   // 等房主位置广播到服务器
  const hostId = await A.evaluate(() => Net.myId);
  const bPosBefore = await B.evaluate(() => [window.Player.pos.x, window.Player.pos.y, window.Player.pos.z]);
  await B.evaluate(id => Net.requestTp(id), hostId);
  for (let i = 0; i < 8; i++){
    await new Promise(r => setTimeout(r, 2000));
    const bp = await B.evaluate(() => [window.Player.pos.x, window.Player.pos.y, window.Player.pos.z]);
    console.log(`[tp diag] before=${bPosBefore.map(v => v.toFixed(1)).join(',')} now=${bp.map(v => v.toFixed(1)).join(',')}`);
    if (Math.abs(bp[0] - 50) < 6 && Math.abs(bp[2] - 50) < 6) break;
  }
  await B.waitForFunction(() => {
    const p = window.Player.pos;
    return Math.abs(p.x - 50) < 6 && Math.abs(p.z - 50) < 6;
  }, null, { timeout: 5000 });
  ok('访客传送到房主身边', true);

  // ---- 昼夜同步（服务器权威时间）----
  const tA = await A.evaluate(() => Net.syncedTime());
  const tB = await B.evaluate(() => Net.syncedTime());
  ok('双方昼夜时间一致', Math.abs(tA - tB) < 0.01, `A=${tA.toFixed(4)} B=${tB.toFixed(4)}`);

  // ---- 世界持久化到磁盘 ----
  await new Promise(r => setTimeout(r, 3000));   // 等 debounce/自动存档落盘
  let worldFile = false, worldJson = null;
  try { await access(join(SAVE, 'world.json')); worldFile = true; worldJson = JSON.parse(await readFile(join(SAVE, 'world.json'), 'utf8')); } catch(e){}
  ok('服务器世界已落盘', worldFile);
  ok('方块改动已入盘', worldFile && worldJson && Object.keys(worldJson.planets['0'].mods).length > 0);

  // ---- 访客断开后房主不受影响 ----
  await B.evaluate(() => Net.disconnect());
  ok('访客断开后房主仍在线', await A.evaluate(() => Net.active()));

  await A.evaluate(() => Net.disconnect());
  await browser.close();
} catch (e){
  console.error('E2E 失败：', e && e.message);
  results.push({ name: 'e2e', pass: false, extra: String(e && e.message) });
  try { await browser.close(); } catch(e2){}
}
srv.kill();
const bad = results.filter(r => !r.pass);
console.log(`\n${results.length - bad.length}/${results.length} passed`);
process.exit(bad.length ? 1 : 0);

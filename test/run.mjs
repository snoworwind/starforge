/* STARFORGE 全自动测试运行器
   用法：
     node test/run.mjs                  # 无头运行全套（用系统 Edge，无需下载浏览器）
     node test/run.mjs --headed         # 有头（可视化调试）
     node test/run.mjs --grep=factory   # 只跑名字匹配的套件
     node test/run.mjs --browser=edge   # 可选 edge|chrome（默认 edge）

   输出：
     test-results.json  — 机器可读结果（AI agent / CI 消费）
     test-results.xml   — JUnit 格式（CI 集成）
     退出码             — 0=全过，1=有失败/异常 */
import { chromium } from 'playwright-core';
import { readdir, readFile, writeFile, mkdir } from 'node:fs/promises';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = join(__dirname, '..');
const TESTS_DIR = join(ROOT, 'tests');
const OUT_DIR = join(ROOT, 'test-results');
const PORT = 17899;
const BASE = `http://127.0.0.1:${PORT}/index.html?test=1`;

// 启动静态服务器（serve.mjs 导入即监听 17899）
await import('./serve.mjs');

function arg(name, fallback) {
  // 支持 --name=value 与 --name value 两种写法
  for (let i = 0; i < process.argv.length; i++) {
    const a = process.argv[i];
    if (a === '--' + name) return process.argv[i + 1] || fallback;
    if (a.startsWith('--' + name + '=')) return a.slice(('--' + name + '=').length);
  }
  return fallback;
}
const HEADED = process.argv.includes('--headed');
const GREP = arg('grep', null);
const BROWSER = arg('browser', 'edge');

const browserTypes = { edge: 'msedge', chrome: 'chrome' };
const channel = browserTypes[BROWSER] || 'msedge';

console.log(`[run] 启动 ${channel}（${HEADED ? '有头' : '无头'}）…`);

const browser = await chromium.launch({
  channel,
  headless: !HEADED,
  args: [
    '--enable-unsafe-swiftshader',
    '--use-gl=angle',
    '--use-angle=swiftshader',
    '--disable-dev-shm-usage',
  ],
});

const context = await browser.newContext({ viewport: { width: 800, height: 600 } });
// 低配画面：软件渲染下最大化降载（区块 6、流畅画质、关闭云/大气/NPC 飞船）
await context.addInitScript(() => {
  try {
    localStorage.setItem('starforge_settings', JSON.stringify({
      fov: 75, chunkDist: 6, farDist: 400, quality: 'low',
      planetLod: 'low', clouds: 'off', realAtmo: 'off', npcShips: 0,
    }));
  } catch (e) {}
});

const page = await context.newPage();
const pageErrors = [];
page.on('pageerror', e => pageErrors.push(String(e && e.message || e)));
page.on('console', msg => { if (msg.type() === 'error') pageErrors.push('[console] ' + msg.text()); });

let result = null;
try {
  await page.goto(BASE, { waitUntil: 'domcontentloaded', timeout: 60000 });
  await page.waitForFunction(() => window.__SF_TEST__ && window.__SF_TEST__.ready, null, { timeout: 120000 });

  // 注入测试套件（文件名排序决定执行顺序）
  const files = (await readdir(TESTS_DIR)).filter(f => f.endsWith('.js')).sort();
  for (const f of files) {
    const src = await readFile(join(TESTS_DIR, f), 'utf8');
    await page.addScriptTag({ content: src });
  }
  console.log(`[run] 已加载 ${files.length} 个测试套件：${files.join(', ')}`);

  result = await page.evaluate((grep) => window.__SF_TEST__.runAll({ grep: grep || null }), GREP);
} catch (e) {
  result = {
    generatedAt: new Date().toISOString(),
    totalMs: 0,
    summary: { suites: 0, tests: 0, passed: 0, failed: 1, ok: false },
    suites: [],
    pageErrors,
    fatal: String(e && e.message || e),
  };
}

await browser.close();

// ---------- 落盘 JSON ----------
await mkdir(OUT_DIR, { recursive: true });
await writeFile(join(OUT_DIR, 'test-results.json'), JSON.stringify(result, null, 2), 'utf8');

// ---------- 落盘 JUnit XML ----------
function esc(s) { return String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;'); }
const xml = ['<?xml version="1.0" encoding="UTF-8"?>', '<testsuites name="starforge">'];
let totalXml = 0, failXml = 0;
for (const s of result.suites || []) {
  xml.push(`  <testsuite name="${esc(s.name)}" tests="${s.tests.length}" failures="${s.failed}">`);
  for (const tc of s.tests) {
    totalXml++; if (!tc.pass) failXml++;
    xml.push(`    <testcase name="${esc(tc.name)}" time="${(tc.ms / 1000).toFixed(3)}">`);
    if (!tc.pass) xml.push(`      <failure message="${esc(tc.error || '')}">${esc(tc.error || '')}</failure>`);
    xml.push('    </testcase>');
  }
  xml.push('  </testsuite>');
}
xml.push('</testsuites>');
await writeFile(join(OUT_DIR, 'test-results.xml'), xml.join('\n') + '\n', 'utf8');

// ---------- 终端摘要 ----------
const sm = result.summary || {};
console.log('\n============================================');
console.log(`  STARFORGE 自动化测试结果`);
console.log(`  套件 ${sm.suites} · 用例 ${sm.tests} · 通过 ${sm.passed} · 失败 ${sm.failed} · ${(result.totalMs / 1000).toFixed(1)}s`);
if (result.fatal) console.log(`  ⚠ 致命错误：${result.fatal}`);
if (result.pageErrors && result.pageErrors.length) {
  console.log('  页面错误（前 10 条）：');
  result.pageErrors.slice(0, 10).forEach(e => console.log('    ' + e));
}
for (const s of result.suites || []) {
  if (s.failed) {
    console.log(`  [✗] ${s.name}（${s.failed} 失败）`);
    for (const tc of s.tests) if (!tc.pass) console.log(`      - ${tc.name}: ${tc.error}`);
  } else {
    console.log(`  [✓] ${s.name}（${s.tests.length}）`);
  }
}
console.log('============================================');
console.log(`结果文件：${join(OUT_DIR, 'test-results.json')}`);
console.log(`JUnit   ：${join(OUT_DIR, 'test-results.xml')}`);

process.exit(result.summary && result.summary.ok ? 0 : 1);

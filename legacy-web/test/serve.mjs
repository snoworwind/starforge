/* STARFORGE 测试用静态文件服务器（Node 原生，无第三方依赖）
   用法：node test/serve.mjs [端口]
   默认端口 17899（避开游戏联机服务器的 17888/17889） */
import http from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, resolve, sep, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, '..');

function resolvePort() {
  for (const v of [process.env.SF_TEST_PORT, process.argv[2]]) {
    const n = Number(v);
    if (Number.isInteger(n) && n > 0 && n < 65536) return n;
  }
  return 17899;
}
const PORT = resolvePort();

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.ico': 'image/x-icon',
  '.svg': 'image/svg+xml',
  '.glb': 'model/gltf-binary',
  '.gltf': 'model/gltf+json',
  '.txt': 'text/plain; charset=utf-8',
};

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url, 'http://localhost');
    let pathname = decodeURIComponent(url.pathname);
    if (pathname === '/') pathname = '/index.html';
    // 路径规范化 + 越界防护（必须落在 ROOT 之内）
    let full = resolve(ROOT, '.' + pathname);
    if (full !== ROOT && !full.startsWith(ROOT + sep)) {
      res.writeHead(403).end('403 Forbidden');
      return;
    }
    const ext = extname(full).toLowerCase();
    const data = await readFile(full);
    res.writeHead(200, {
      'Content-Type': MIME[ext] || 'application/octet-stream',
      'Cache-Control': 'no-cache',
      'Access-Control-Allow-Origin': '*',
    });
    res.end(data);
  } catch (e) {
    res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
    res.end('404 Not Found');
  }
});

server.listen(PORT, '127.0.0.1', () => {
  console.log(`[serve] STARFORGE test server: http://127.0.0.1:${PORT} (root: ${ROOT})`);
});

export { server, PORT, ROOT };

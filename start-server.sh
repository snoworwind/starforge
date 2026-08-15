#!/usr/bin/env sh
# STARFORGE 联机服务器启动脚本（macOS / Linux）
# 用法：./start-server.sh [参数]
# 参数透传给 server.mjs，例如：./start-server.sh --name "我的服务器" --password 123
cd "$(dirname "$0")" || exit 1
if ! command -v node >/dev/null 2>&1; then
  echo "[!] 未检测到 Node.js，请先安装：https://nodejs.org/"
  exit 1
fi
echo "Starting STARFORGE server...  (Ctrl+C 停止)"
exec node server.mjs "$@"

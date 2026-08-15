@echo off
setlocal
cd /d "%~dp0"
where node >nul 2>nul
if errorlevel 1 (
  echo [!] 未检测到 Node.js，请先安装：https://nodejs.org/
  echo     安装后重新双击本脚本即可启动联机服务器。
  pause
  exit /b 1
)
echo Starting STARFORGE server...  ^(Ctrl+C 停止^)
echo.
node server.mjs %*
if errorlevel 1 (
  echo.
  echo [!] 服务器异常退出（端口被占用？可编辑 server-config.json 换端口）。
  pause
)

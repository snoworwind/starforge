@echo off
rem STARFORGE 联机主机启动（跨平台服务器入口：内部使用 Node.js，见 start-server.bat）
cd /d "%~dp0"
call start-server.bat %*

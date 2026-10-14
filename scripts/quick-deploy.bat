@echo off
setlocal
cd /d "%~dp0.."
where node >nul 2>&1
if errorlevel 1 echo quick-deploy: node is required 1>&2 & exit /b 1
node "%~dp0quick-deploy.mjs" %*
exit /b %ERRORLEVEL%

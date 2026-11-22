@rem Velella Codex update guard v1
@echo off
node "%~dp0codex-npm.cjs" %*
exit /b %errorlevel%

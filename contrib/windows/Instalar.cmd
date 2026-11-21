@echo off
rem Abre o install.ps1 sem depender da política de execução do PowerShell.
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1"
pause

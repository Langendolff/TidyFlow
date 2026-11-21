# Remove o TidyFlow instalado pelo install.ps1. O config e o histórico ficam em
# %APPDATA%\TidyFlow (apague à mão se quiser).
$ErrorActionPreference = 'Continue'
$dest = Join-Path $env:LOCALAPPDATA 'Programs\TidyFlow'
$bin = Join-Path $dest 'bin'
$tidyflow = Join-Path $bin 'tidyflow.exe'

if (Test-Path $tidyflow) {
  & $tidyflow service disable | Out-Null
  & $tidyflow browser uninstall | Out-Null
}
Get-Process tidyflowd, tidyflow-app -ErrorAction SilentlyContinue | Stop-Process -Force
[Microsoft.Win32.Registry]::CurrentUser.DeleteSubKeyTree('Software\Classes\*\shell\TidyFlow', $false)
Remove-Item (Join-Path ([Environment]::GetFolderPath('Programs')) 'TidyFlow.lnk') -ErrorAction SilentlyContinue

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath) {
  $kept = ($userPath -split ';') | Where-Object { $_ -and $_ -ne $bin }
  [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
}
Set-Location $env:TEMP
Remove-Item $dest -Recurse -Force -ErrorAction SilentlyContinue
"TidyFlow removido. Config e histórico continuam em $(Join-Path $env:APPDATA 'TidyFlow')."

# Instala o TidyFlow a partir do pacote de release (TidyFlow-<versão>-windows-x64.zip).
# Uso: dois cliques em "Instalar.cmd", ou no PowerShell:
#   powershell -ExecutionPolicy Bypass -File .\install.ps1
# Não liga o daemon: revise as regras antes (o exemplo manda para a lixeira o que tem
# mais de 30 dias). Depois: tidyflow service enable
$ErrorActionPreference = 'Stop'
$src = $PSScriptRoot
$dest = Join-Path $env:LOCALAPPDATA 'Programs\TidyFlow'
$bin = Join-Path $dest 'bin'

# Um daemon ou app aberto segura os .exe.
Get-Process tidyflowd, tidyflow, tidyflow-app -ErrorAction SilentlyContinue | Stop-Process -Force
New-Item -ItemType Directory -Force $bin | Out-Null
Copy-Item (Join-Path $src 'bin\*.exe') $bin -Force
foreach ($f in 'README.md', 'LICENSE', 'config.example.toml', 'uninstall.ps1') {
  Copy-Item (Join-Path $src $f) $dest -Force
}
# Tira a marca de "baixado da internet": sem isso o SmartScreen pergunta a cada execução.
Get-ChildItem $dest -Recurse -File | Unblock-File
"programa em $dest"

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not $userPath) { $userPath = '' }
if (($userPath -split ';') -notcontains $bin) {
  [Environment]::SetEnvironmentVariable('Path', (($userPath.TrimEnd(';'), $bin) -join ';').TrimStart(';'), 'User')
  "$bin adicionado ao PATH (abra um terminal novo)"
}

$cfgDir = Join-Path $env:APPDATA 'TidyFlow'
$cfg = Join-Path $cfgDir 'config.toml'
if (Test-Path $cfg) {
  "config mantido: $cfg"
} else {
  New-Item -ItemType Directory -Force $cfgDir | Out-Null
  Copy-Item (Join-Path $src 'config.example.toml') $cfg
  "config de exemplo copiado para $cfg"
}

$shell = New-Object -ComObject WScript.Shell
$lnk = $shell.CreateShortcut((Join-Path ([Environment]::GetFolderPath('Programs')) 'TidyFlow.lnk'))
$lnk.TargetPath = Join-Path $bin 'tidyflow-app.exe'
$lnk.WorkingDirectory = $bin
$lnk.Description = 'Organiza arquivos automaticamente por regras'
$lnk.Save()
'atalho criado no Menu Iniciar'

& (Join-Path $bin 'tidyflow.exe') browser install | Out-Null
if ($LASTEXITCODE -eq 0) {
  'host da extensão de navegador registrado'
} else {
  'aviso: não foi possível registrar o host da extensão (rode: tidyflow browser install)'
}

# Menu de contexto do Explorer ("Mostrar mais opções" no Windows 11).
$key = [Microsoft.Win32.Registry]::CurrentUser.CreateSubKey('Software\Classes\*\shell\TidyFlow')
$key.SetValue('MUIVerb', 'Organizar com o TidyFlow')
$key.SetValue('Icon', (Join-Path $bin 'tidyflow-app.exe'))
$cmd = $key.CreateSubKey('command')
$cmd.SetValue('', ('"{0}" organize "%1"' -f (Join-Path $bin 'tidyflowd.exe')))
$cmd.Close(); $key.Close()
'menu de contexto do Explorer instalado'

@"

Pronto. Próximos passos:
  1. Revise as regras em $cfg
  2. tidyflow check; tidyflow run --dry-run
  3. tidyflow service enable   (liga o daemon no login)
  4. Opcional: instale a extensão de navegador (veja o README)
"@

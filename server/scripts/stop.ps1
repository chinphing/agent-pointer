$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$PidFile = Join-Path $Root '.pointer-server.pid'

if (-not (Test-Path $PidFile)) {
  Write-Host '[pointer-server] Not running (no PID file).'
  exit 0
}

$pid = (Get-Content $PidFile -Raw).Trim()
if (-not $pid -or -not (Get-Process -Id $pid -ErrorAction SilentlyContinue)) {
  Write-Host "[pointer-server] Stale PID file ($pid); removing."
  Remove-Item $PidFile -Force
  exit 0
}

Stop-Process -Id $pid -Force -ErrorAction SilentlyContinue
Remove-Item $PidFile -Force
Write-Host "[pointer-server] Stopped (PID $pid)."

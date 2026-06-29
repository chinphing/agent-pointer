$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$PidFile = Join-Path $Root '.pointer-server.pid'
$LogFile = Join-Path $Root 'pointer-server.log'

if (-not (Test-Path $PidFile)) {
  Write-Host '[pointer-server] Status: stopped'
  exit 1
}

$pid = (Get-Content $PidFile -Raw).Trim()
if (-not $pid -or -not (Get-Process -Id $pid -ErrorAction SilentlyContinue)) {
  Write-Host "[pointer-server] Status: stopped (stale PID $pid)"
  Remove-Item $PidFile -Force -ErrorAction SilentlyContinue
  exit 1
}

Write-Host "[pointer-server] Status: running (PID $pid)"
Write-Host "[pointer-server] Log: $LogFile"

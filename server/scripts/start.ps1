# Start pointer-server in the background (same directory as this script).
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
$Bin = Join-Path $Root 'pointer-server.exe'
$PidFile = Join-Path $Root '.pointer-server.pid'
$LogFile = Join-Path $Root 'pointer-server.log'

if (-not (Test-Path $Bin)) {
  Write-Error "[pointer-server] Binary not found: $Bin"
}

if (Test-Path $PidFile) {
  $existingPid = (Get-Content $PidFile -Raw).Trim()
  if ($existingPid -and (Get-Process -Id $existingPid -ErrorAction SilentlyContinue)) {
    Write-Host "[pointer-server] Already running (PID $existingPid)."
    exit 0
  }
  Remove-Item $PidFile -Force
}

$proc = Start-Process -FilePath $Bin -WorkingDirectory $Root -WindowStyle Hidden `
  -RedirectStandardOutput $LogFile -RedirectStandardError $LogFile -PassThru
Set-Content -Path $PidFile -Value $proc.Id -NoNewline
Write-Host "[pointer-server] Started (PID $($proc.Id))."
Write-Host "[pointer-server] Log: $LogFile"

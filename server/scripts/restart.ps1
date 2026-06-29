$Root = Split-Path -Parent $MyInvocation.MyCommand.Path
& (Join-Path $Root 'stop.ps1')
& (Join-Path $Root 'start.ps1')

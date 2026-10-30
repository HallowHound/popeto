# Build Popeto and assemble the portable folder: dist\Popeto (Popeto.exe, README.txt, pranks\, empty img\).
# Usage:  powershell -ExecutionPolicy Bypass -File scripts\package.ps1
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    cargo build --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $out = Join-Path $root 'dist\Popeto'
    if (Test-Path $out) { Remove-Item -Recurse -Force $out }
    New-Item -ItemType Directory -Force (Join-Path $out 'img') | Out-Null
    Copy-Item 'target\release\popeto.exe' (Join-Path $out 'Popeto.exe')
    Copy-Item 'assets\README.txt' $out
    Copy-Item -Recurse 'assets\pranks' $out
    Write-Host "Packaged to $out - put Shimeji pets into img\ (or use tray > Add pets from Shimeji zip)."
}
finally { Pop-Location }

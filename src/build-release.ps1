[CmdletBinding()]
param(
    [string]$InstallDir = (Join-Path $env:USERPROFILE '.cargo\bin'),
    [switch]$StopRunning,
    [switch]$SkipInstall
)

$ErrorActionPreference = 'Stop'

$projectRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$manifestPath = Join-Path $projectRoot 'Cargo.toml'
$binaryName = 'opencode-tui-rust.exe'
$releaseBinary = Join-Path $projectRoot "target\release\$binaryName"
$InstallDir = [System.IO.Path]::GetFullPath($InstallDir)
$installedBinary = Join-Path $InstallDir $binaryName

if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "Cargo.toml was not found: $manifestPath"
}

$cargo = Get-Command cargo.exe -ErrorAction SilentlyContinue
if (-not $cargo) {
    throw 'cargo.exe was not found on PATH. Install Rust or add .cargo\bin to PATH.'
}

Write-Host "Building release binary in $projectRoot"
Push-Location $projectRoot
try {
    & $cargo.Source build --release --locked
    if ($LASTEXITCODE -ne 0) {
        throw "Cargo release build failed with exit code $LASTEXITCODE"
    }
}
finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $releaseBinary -PathType Leaf)) {
    throw "Release binary was not produced: $releaseBinary"
}

$releaseHash = (Get-FileHash -LiteralPath $releaseBinary -Algorithm SHA256).Hash
Write-Host "Release binary: $releaseBinary"
Write-Host "Release SHA-256: $releaseHash"

if ($SkipInstall) {
    Write-Host 'Build completed. Installation was skipped.'
    exit 0
}

if (-not (Test-Path -LiteralPath $InstallDir -PathType Container)) {
    New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
}

$running = @(Get-Process -Name 'opencode-tui-rust' -ErrorAction SilentlyContinue | Where-Object {
    try {
        $_.Path -and ([System.IO.Path]::GetFullPath($_.Path) -ieq $installedBinary)
    }
    catch {
        $false
    }
})
if ($running.Count -gt 0) {
    if (-not $StopRunning) {
        $ids = ($running | ForEach-Object { $_.Id }) -join ', '
        throw "opencode-tui-rust is running (PID: $ids). Close it or rerun with -StopRunning."
    }

    foreach ($process in $running) {
        Write-Host "Stopping opencode-tui-rust (PID $($process.Id))"
        Stop-Process -Id $process.Id -ErrorAction Stop
    }

    Start-Sleep -Milliseconds 300
    $stillRunning = @(Get-Process -Name 'opencode-tui-rust' -ErrorAction SilentlyContinue | Where-Object {
        try {
            $_.Path -and ([System.IO.Path]::GetFullPath($_.Path) -ieq $installedBinary)
        }
        catch {
            $false
        }
    })
    if ($stillRunning.Count -gt 0) {
        throw 'opencode-tui-rust did not stop in time; the installed binary was not replaced.'
    }
}

$stagedBinary = Join-Path $InstallDir "$binaryName.$PID.tmp"
try {
    Copy-Item -LiteralPath $releaseBinary -Destination $stagedBinary -Force

    if (Test-Path -LiteralPath $installedBinary -PathType Leaf) {
        $backupBinary = Join-Path $InstallDir "$binaryName.$PID.bak"
        if (Test-Path -LiteralPath $backupBinary) {
            Remove-Item -LiteralPath $backupBinary -Force
        }

        try {
            [System.IO.File]::Replace($stagedBinary, $installedBinary, $backupBinary, $true)
        }
        finally {
            if (Test-Path -LiteralPath $backupBinary) {
                Remove-Item -LiteralPath $backupBinary -Force -ErrorAction SilentlyContinue
            }
        }
    }
    else {
        Move-Item -LiteralPath $stagedBinary -Destination $installedBinary -Force
    }
}
finally {
    if (Test-Path -LiteralPath $stagedBinary) {
        Remove-Item -LiteralPath $stagedBinary -Force -ErrorAction SilentlyContinue
    }
}

$installedHash = (Get-FileHash -LiteralPath $installedBinary -Algorithm SHA256).Hash
if ($installedHash -ne $releaseHash) {
    throw "Installed binary hash does not match the release build: $installedBinary"
}

Write-Host "Installed: $installedBinary"
Write-Host "Installed SHA-256: $installedHash"

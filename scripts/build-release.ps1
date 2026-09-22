$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $repo
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) { throw 'Windows is required.' }
if (-not [Environment]::Is64BitOperatingSystem) { throw 'Windows x64 is required.' }

function Invoke-Checked([string]$name, [string[]]$arguments) {
    & $name @arguments
    if ($LASTEXITCODE -ne 0) { throw "$name failed with exit code $LASTEXITCODE" }
}

$metadataText = & cargo metadata --no-deps --format-version 1
if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed.' }
$metadata = $metadataText | ConvertFrom-Json
$package = $metadata.packages | Where-Object { $_.name -eq 'logipeek' } | Select-Object -First 1
if (-not $package) { throw 'The logipeek package was not found.' }
$version = $package.version
if ($version -notmatch '^\d+\.\d+\.\d+$') { throw "Unexpected package version: $version" }

$iscc = $env:INNO_SETUP_ISCC
if (-not $iscc) {
    $candidate = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($candidate) { $iscc = $candidate.Source }
}
if (-not $iscc) {
    $locations = @(
        'D:\DevTools\Inno Setup 7\ISCC.exe',
        'D:\DevTools\InnoSetup\ISCC.exe',
        "${env:ProgramFiles}\Inno Setup 7\ISCC.exe",
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
    )
    $iscc = $locations | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
}
if (-not $iscc -or -not (Test-Path -LiteralPath $iscc)) {
    throw 'Inno Setup ISCC.exe is required. Install official Inno Setup or set INNO_SETUP_ISCC.'
}

Invoke-Checked cargo @('fmt', '--check')
Invoke-Checked cargo @('check')
Invoke-Checked cargo @('test')
Invoke-Checked cargo @('clippy', '--all-targets', '--all-features', '--', '-D', 'warnings')
$previousRustFlags = $env:RUSTFLAGS
try {
    $env:RUSTFLAGS = (($previousRustFlags, '-C target-feature=+crt-static') | Where-Object { $_ }) -join ' '
    Invoke-Checked cargo @('build', '--release')
} finally {
    $env:RUSTFLAGS = $previousRustFlags
}
Invoke-Checked git @('diff', '--check')

$exe = Join-Path $repo 'target\release\logipeek.exe'
$icon = Join-Path $repo 'assets\logipeek.ico'
foreach ($file in @($exe, $icon, (Join-Path $repo 'LICENSE'))) {
    if (-not (Test-Path -LiteralPath $file)) { throw "Missing release input: $file" }
}
$dist = Join-Path $repo 'dist'
if (Test-Path -LiteralPath $dist) { Remove-Item -LiteralPath $dist -Recurse -Force }
New-Item -ItemType Directory -Path $dist | Out-Null

$previousVersion = $env:LOGIPEEK_VERSION
try {
    $env:LOGIPEEK_VERSION = $version
    Invoke-Checked $iscc @((Join-Path $repo 'installer\LogiPeek.iss'))
} finally {
    $env:LOGIPEEK_VERSION = $previousVersion
}

$installer = Join-Path $dist "LogiPeek-$version-x64-Setup.exe"
if (-not (Test-Path -LiteralPath $installer)) { throw 'Installer was not produced.' }
$stage = Join-Path $dist '.portable-stage'
New-Item -ItemType Directory -Path $stage | Out-Null
try {
    Copy-Item -LiteralPath $exe -Destination (Join-Path $stage 'logipeek.exe')
    foreach ($name in @('LICENSE', 'README.md', 'README.zh-CN.md')) {
        Copy-Item -LiteralPath (Join-Path $repo $name) -Destination (Join-Path $stage $name)
    }
    $portable = Join-Path $dist "LogiPeek-$version-x64-portable.zip"
    Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $portable -CompressionLevel Optimal
} finally {
    Remove-Item -LiteralPath $stage -Recurse -Force
}

$artifacts = @($installer, $portable)
$hashes = foreach ($file in $artifacts) {
    $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
    "$hash  $(Split-Path -Leaf $file)"
}
$hashes | Set-Content -LiteralPath (Join-Path $dist 'SHA256SUMS.txt') -Encoding ascii
foreach ($file in @($artifacts + (Join-Path $dist 'SHA256SUMS.txt'))) {
    $item = Get-Item -LiteralPath $file
    $hash = (Get-FileHash -LiteralPath $file -Algorithm SHA256).Hash.ToLowerInvariant()
    '{0}  {1:N0} bytes  SHA256 {2}' -f $item.Name, $item.Length, $hash
}

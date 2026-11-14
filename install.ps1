#Requires -Version 5.1
<#
.SYNOPSIS
  freepi-bridge installer for Windows.

.DESCRIPTION
  Downloads the release build, verifies its SHA-256, installs it under
  %LOCALAPPDATA%\Programs\freepi-bridge, and adds that folder to the user PATH.

.EXAMPLE
  irm https://raw.githubusercontent.com/ihsan-ramadhan/freepi-bridge/main/install.ps1 | iex

.EXAMPLE
  .\install.ps1 -Prefix C:\tools\freepi-bridge -Version v0.1.0

.EXAMPLE
  .\install.ps1 -Uninstall

.NOTES
  When piped into iex there are no arguments; use the FREEPI_BRIDGE_PREFIX,
  FREEPI_BRIDGE_VERSION, FREEPI_BRIDGE_DOWNLOAD_BASE and
  FREEPI_BRIDGE_UNINSTALL environment variables.
#>
param(
  [string]$Prefix = $(if ($env:FREEPI_BRIDGE_PREFIX) { $env:FREEPI_BRIDGE_PREFIX } else { Join-Path $env:LOCALAPPDATA 'Programs\freepi-bridge' }),
  [string]$Version = $env:FREEPI_BRIDGE_VERSION,
  [switch]$FromSource,
  [switch]$NoPath,
  [switch]$Uninstall = [bool]($env:FREEPI_BRIDGE_UNINSTALL -eq '1'),
  [switch]$KeepState
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$repo = 'ihsan-ramadhan/freepi-bridge'
$base = if ($env:FREEPI_BRIDGE_DOWNLOAD_BASE) { $env:FREEPI_BRIDGE_DOWNLOAD_BASE } else { "https://github.com/$repo/releases" }
$asset = 'freepi-bridge-windows-x86_64.zip'
$url = if ($Version) { "$base/download/$Version/$asset" } else { "$base/latest/download/$asset" }

function Write-Step([string]$Text) { Write-Host $Text }

function Add-ToUserPath([string]$Directory) {
  if ($NoPath) { return }
  $current = [Environment]::GetEnvironmentVariable('Path', 'User')
  $entries = @()
  if ($current) { $entries = $current -split ';' | Where-Object { $_ } }
  if ($entries -notcontains $Directory) {
    $updated = (@($entries) + $Directory) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
    Write-Step "added $Directory to the user PATH (open a new shell to pick it up)"
  }
  $env:Path = "$Directory;$env:Path"
}

function Remove-FromUserPath([string]$Directory) {
  $current = [Environment]::GetEnvironmentVariable('Path', 'User')
  if (-not $current) { return }
  $entries = @($current -split ';' | Where-Object { $_ })
# remove the single entry the installer added; leave any other duplicates alone
  $removed = $false
  $kept = foreach ($entry in $entries) {
    if (-not $removed -and ($entry.TrimEnd('\')) -eq ($Directory.TrimEnd('\'))) {
      $removed = $true
      continue
    }
    $entry
  }
  if (-not $removed) {
    Write-Step "$Directory was not on the user PATH"
    return
  }
  [Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')
  Write-Step "removed $Directory from the user PATH"
}

function Remove-Install([string]$StateDir) {
  $binary = Join-Path $Prefix 'freepi-bridge.exe'
  if (Test-Path $binary) {
    # stop the daemon recorded in state.json before its binary disappears
    try { & $binary stop | Out-Null } catch { }
  }

  if (Test-Path $Prefix) {
    Remove-Item -Recurse -Force $Prefix -ErrorAction SilentlyContinue
    if (Test-Path $Prefix) { throw "could not remove $Prefix" }
    Write-Step "removed $Prefix"
  } else {
    Write-Step "nothing installed at $Prefix"
  }

  Remove-FromUserPath $Prefix

  if ($KeepState) {
    Write-Step "kept $StateDir (-KeepState)"
    return
  }
  if (Test-Path $StateDir) {
    Remove-Item -Recurse -Force $StateDir -ErrorAction SilentlyContinue
    if (Test-Path $StateDir) { throw "could not remove $StateDir" }
    Write-Step "removed $StateDir"
  }
}

function Get-Release([string]$TempDir) {
  $archive = Join-Path $TempDir $asset
  Write-Step "downloading $asset ..."
  Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile $archive

  $expected = $null
  try {
    $checksum = (Invoke-WebRequest -UseBasicParsing -Uri "$url.sha256").Content
    $expected = ($checksum -split '\s+')[0].ToLower()
  } catch {
    Write-Step 'no checksum file published; skipping verification'
  }
  if ($expected) {
    $actual = (Get-FileHash -Algorithm SHA256 -Path $archive).Hash.ToLower()
    if ($actual -ne $expected) { throw "checksum mismatch for $asset" }
    Write-Step 'checksum verified'
  }

  Expand-Archive -Path $archive -DestinationPath $TempDir -Force
  $binary = Join-Path $TempDir 'freepi-bridge.exe'
  if (-not (Test-Path $binary)) { throw 'release archive has no freepi-bridge.exe' }
  return $binary
}

function Build-FromSource([string]$TempDir) {
  if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw 'cargo not found; install Rust from https://rustup.rs or download a release manually'
  }
  Write-Step "cloning $repo ..."
  $source = Join-Path $TempDir 'src'
  git clone --depth 1 "https://github.com/$repo.git" $source
  Push-Location $source
  try {
    Write-Step 'building release binary ...'
    cargo build --release --locked
  } finally {
    Pop-Location
  }
  return (Join-Path $source 'target\release\freepi-bridge.exe')
}

if ($Uninstall) {
  $stateDir = if ($env:FREEPI_BRIDGE_DIR) { $env:FREEPI_BRIDGE_DIR } else { Join-Path $env:USERPROFILE '.freepi-bridge' }
  Remove-Install $stateDir
  Write-Step 'freepi-bridge uninstalled; ~/.free-pi (the official CLI) was left alone'
  return
}

New-Item -ItemType Directory -Force -Path $Prefix | Out-Null
$temp = Join-Path ([IO.Path]::GetTempPath()) ("freepi-bridge-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Force -Path $temp | Out-Null

try {
  $binary = $null
  if (-not $FromSource) {
    try {
      $binary = Get-Release $temp
    } catch {
      Write-Step "release install failed: $($_.Exception.Message)"
      Write-Step 'falling back to a source build'
    }
  }
  if (-not $binary) {
    $binary = Build-FromSource $temp
  }

  $destination = Join-Path $Prefix 'freepi-bridge.exe'
  Copy-Item -Path $binary -Destination $destination -Force
  Write-Step "installed $destination"
  Add-ToUserPath $Prefix
  & $destination --version
} finally {
  Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue
}

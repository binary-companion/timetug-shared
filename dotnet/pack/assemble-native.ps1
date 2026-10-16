<#
.SYNOPSIS
  Collects the native set of the TimeTug engine for one Windows architecture: TimeTugEngineC.dll from
  `swift build --show-bin-path` plus exactly the Swift runtime DLLs it needs, into one flat directory.

.DESCRIPTION
  Used by the dotnet CI jobs (the set is copied next to the C# test binary) and by the publish-nuget job of
  release.yml (the set becomes runtimes/<rid>/native in the package). It does not build: run
  `swift build -c release --product TimeTugEngineC` first.

  1. Checks that the DLL exists, is built for the requested architecture, and exports the four tt_engine_*
     functions (`dumpbin /exports`).
  2. Walks `dumpbin /dependents` recursively from the DLL. A dependency found in the bin directory or in the Swift
     runtime directory is copied and walked in turn. The Visual C++ runtime (MSVCP140*, VCRUNTIME140*), the
     universal CRT and Windows system DLLs are not bundled: an app takes them from the Visual C++ runtime and from
     Windows. Any other dependency found nowhere fails the script.

  dumpbin comes from the newest Visual Studio (found with vswhere) or from the PATH.
  Exits non-zero, with a message, on any failure.

.EXAMPLE
  pwsh dotnet/pack/assemble-native.ps1 -Rid win-x64 -Out build/native/win-x64
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [ValidateSet('win-x64', 'win-arm64')] [string] $Rid,
    [Parameter(Mandatory)] [string] $Out,
    [string] $Configuration = 'release',
    # The Swift runtime's bin directory; default: the PATH entry that holds swiftCore.dll.
    [string] $RuntimeDir = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$exports = @('tt_engine_create', 'tt_engine_send', 'tt_engine_reply', 'tt_engine_destroy')

function Get-Machine([string] $Path) {
    # IMAGE_FILE_HEADER.Machine, after the PE signature that e_lfanew (offset 0x3C) points at.
    $bytes = [System.IO.File]::ReadAllBytes($Path)
    $peOffset = [BitConverter]::ToInt32($bytes, 0x3C)
    if ([BitConverter]::ToUInt32($bytes, $peOffset) -ne 0x00004550) { throw "$Path is not a PE image" }
    switch ([BitConverter]::ToUInt16($bytes, $peOffset + 4)) {
        0x8664 { return 'win-x64' }
        0xAA64 { return 'win-arm64' }
        default { return ('machine 0x{0:X4}' -f $_) }
    }
}

function Find-Dumpbin {
    $onPath = Get-Command dumpbin.exe -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path -LiteralPath $vswhere)) { throw 'dumpbin.exe is not on the PATH and vswhere.exe was not found' }
    $found = @(& $vswhere -latest -products * -find 'VC\Tools\MSVC\**\bin\Host*\*\dumpbin.exe')
    if ($found.Count -eq 0) { throw 'vswhere found no dumpbin.exe (the MSVC tools are not installed)' }
    $hostArch = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { 'arm64' } else { 'x64' }
    $native = $found | Where-Object { $_ -match "\\Host$hostArch\\$hostArch\\dumpbin\.exe$" } | Select-Object -First 1
    if ($native) { return $native }
    return $found[0]
}

function Invoke-Dumpbin([string] $Dumpbin, [string] $Option, [string] $Path) {
    $output = & $Dumpbin /nologo $Option $Path
    if ($LASTEXITCODE -ne 0) { throw "dumpbin $Option $Path exited with code $LASTEXITCODE" }
    return $output
}

function Get-Dependents([string] $Dumpbin, [string] $Path) {
    # The names under "Image has the following [delay load ]dependencies:", one per indented line.
    $names = @()
    $inList = $false
    foreach ($line in (Invoke-Dumpbin $Dumpbin '/dependents' $Path)) {
        if ($line -match 'Image has the following (delay load )?dependencies') { $inList = $true; continue }
        if ($inList -and $line -match '^\s+(\S+\.dll)\s*$') { $names += $Matches[1]; continue }
        if ($inList -and $line -match '\S') { $inList = $false }
    }
    return $names
}

function Test-NotBundled([string] $Name) {
    return $Name -match '^(api-ms-win-|ext-ms-)' -or $Name -match '^(msvcp140|vcruntime140|ucrtbase|concrt140|vccorlib140)'
}

try {
    Set-Location (Join-Path $PSScriptRoot '..\..')
    $swift = Get-Command swift -ErrorAction SilentlyContinue
    if ($null -eq $swift) { throw 'swift is not on the PATH (run scripts/ci/install-swift-windows.ps1 first)' }
    $bin = (& swift build -c $Configuration --show-bin-path | Select-Object -Last 1)
    if ($LASTEXITCODE -ne 0 -or -not $bin) { throw "swift build --show-bin-path exited with code $LASTEXITCODE" }
    $bin = $bin.Trim()
    $dll = Join-Path $bin 'TimeTugEngineC.dll'
    if (-not (Test-Path -LiteralPath $dll)) {
        throw "TimeTugEngineC.dll not found in $bin; build it first: swift build -c $Configuration --product TimeTugEngineC"
    }
    $machine = Get-Machine $dll
    if ($machine -ne $Rid) { throw "$dll is built for $machine, not $Rid" }

    if (-not $RuntimeDir) {
        $RuntimeDir = $env:Path -split ';' | Where-Object { $_ -and (Test-Path -LiteralPath (Join-Path $_ 'swiftCore.dll')) } |
            Select-Object -First 1
        if (-not $RuntimeDir) { throw 'no PATH entry holds swiftCore.dll; pass -RuntimeDir' }
    }
    if (-not (Test-Path -LiteralPath (Join-Path $RuntimeDir 'swiftCore.dll'))) { throw "swiftCore.dll not found in $RuntimeDir" }

    $dumpbin = Find-Dumpbin
    Write-Host "dumpbin: $dumpbin"
    Write-Host "bin: $bin"
    Write-Host "Swift runtime: $RuntimeDir"

    $exported = Invoke-Dumpbin $dumpbin '/exports' $dll
    $missing = $exports | Where-Object { -not ($exported -match "\s$_\s*$") }
    if ($missing) { throw "$dll does not export: $($missing -join ', ')" }
    Write-Host "exports: $($exports -join ' ')"

    $system = Join-Path $env:SystemRoot 'System32'
    New-Item -ItemType Directory -Force -Path $Out | Out-Null
    $copied = @{}
    $queue = [System.Collections.Generic.Queue[string]]::new()
    Copy-Item -LiteralPath $dll -Destination $Out -Force
    $copied['timetugenginec.dll'] = $dll
    $queue.Enqueue($dll)
    while ($queue.Count -gt 0) {
        $image = $queue.Dequeue()
        foreach ($name in (Get-Dependents $dumpbin $image)) {
            $key = $name.ToLowerInvariant()
            if ($copied.ContainsKey($key) -or (Test-NotBundled $name)) { continue }
            $source = @((Join-Path $bin $name), (Join-Path $RuntimeDir $name)) |
                Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
            if ($source) {
                Copy-Item -LiteralPath $source -Destination $Out -Force
                $copied[$key] = $source
                $queue.Enqueue($source)
            } elseif (Test-Path -LiteralPath (Join-Path $system $name)) {
                continue
            } else {
                throw "$name (needed by $(Split-Path -Leaf $image)) is in neither $bin, $RuntimeDir nor $system"
            }
        }
    }

    $total = 0
    Get-ChildItem -LiteralPath $Out -File | Sort-Object Name | ForEach-Object {
        $total += $_.Length
        Write-Host ('{0,12:N0}  {1}' -f $_.Length, $_.Name)
    }
    Write-Host ('{0,12:N0}  total, {1} files for {2} in {3}' -f $total, $copied.Count, $Rid, (Resolve-Path $Out))
} catch {
    Write-Host "::error::assemble-native.ps1: $($_.Exception.Message)"
    exit 1
}

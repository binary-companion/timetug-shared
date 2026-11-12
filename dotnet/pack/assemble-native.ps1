<#
.SYNOPSIS
  Collects the native set of the TimeTug engine for one Windows architecture into one flat directory: the single
  TimeTugEngineC.dll that cargo built.

.DESCRIPTION
  Used by the dotnet CI jobs (the set is copied next to the C# test binary) and by the nuget-native job of
  release.yml (the set becomes runtimes/<rid>/native in the package). It does not build: run
  `cargo build --release --locked -p timetug-engine-c --target <triple>` first.

  1. Takes target/<triple>/release/TimeTugEngineC.dll (triples x86_64-pc-windows-msvc and aarch64-pc-windows-msvc,
     under $env:CARGO_TARGET_DIR when set), checks that it is built for the requested architecture and exports the
     four tt_engine_* functions, and copies it, alone, to -Out.
  2. Checks `dumpbin /dependents` (static and delay-load): every dependency must match $AllowedDependencies, the
     Windows system DLLs. VCRUNTIME*.dll and MSVCP*.dll are named in the error when present: they would mean the C
     runtime is not linked statically (.cargo/config.toml). Anything else fails; the fix is to find the crate that
     pulled the dependency in, not to extend the list without review.

  dumpbin comes from the newest Visual Studio (found with vswhere) or from the PATH.
  Exits non-zero, with a message, on any failure.

.EXAMPLE
  pwsh dotnet/pack/assemble-native.ps1 -Rid win-x64 -Out build/native/win-x64
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [ValidateSet('win-x64', 'win-arm64')] [string] $Rid,
    [Parameter(Mandatory)] [string] $Out
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$exports = @('tt_engine_create', 'tt_engine_send', 'tt_engine_reply', 'tt_engine_destroy')

# The library may depend on these Windows system DLLs and nothing else (matched case-insensitively against the
# whole name). Do not extend the list without review: find the crate that pulled the dependency in.
$AllowedDependencies = @(
    '^KERNEL32\.dll$', '^ntdll\.dll$', '^bcryptprimitives\.dll$', '^ADVAPI32\.dll$', '^USERENV\.dll$',
    '^WS2_32\.dll$', '^api-ms-win-.*\.dll$', '^bcrypt\.dll$'
)
# Dependencies that prove the C runtime is not linked statically.
$DynamicRuntime = '^(VCRUNTIME|MSVCP|UCRTBASE|CONCRT|VCCORLIB|api-ms-win-crt-)'
$Triples = @{ 'win-x64' = 'x86_64-pc-windows-msvc'; 'win-arm64' = 'aarch64-pc-windows-msvc' }

function Test-AllowedDependency([string] $Name) {
    foreach ($pattern in $AllowedDependencies) {
        if ($Name -match $pattern) { return $true }
    }
    return $false
}

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

function Invoke-RustAssembly {
    $triple = $Triples[$Rid]
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    $bin = Join-Path (Join-Path $targetDir $triple) 'release'
    $dll = Join-Path $bin 'TimeTugEngineC.dll'
    if (-not (Test-Path -LiteralPath $dll)) {
        throw "TimeTugEngineC.dll not found in $bin; build it first: cargo build --release --locked -p timetug-engine-c --target $triple"
    }
    $machine = Get-Machine $dll
    if ($machine -ne $Rid) { throw "$dll is built for $machine, not $Rid" }

    $dumpbin = Find-Dumpbin
    Write-Host "dumpbin: $dumpbin"
    Write-Host "library: $dll"

    $exported = Invoke-Dumpbin $dumpbin '/exports' $dll
    $missing = $exports | Where-Object { -not ($exported -match "\s$_\s*$") }
    if ($missing) { throw "$dll does not export: $($missing -join ', ')" }
    Write-Host "exports: $($exports -join ' ')"

    $dependents = @(Get-Dependents $dumpbin $dll)
    Write-Host "dependents: $($dependents -join ' ')"
    # Every Rust DLL imports at least kernel32.dll: an empty list means the dumpbin output was not understood.
    if ($dependents.Count -eq 0) { throw "dumpbin /dependents listed no dependencies for $dll; the dependency checks cannot be trusted" }
    $runtime = @($dependents | Where-Object { $_ -match $DynamicRuntime })
    if ($runtime.Count -gt 0) {
        throw "$dll depends on the dynamic C runtime ($($runtime -join ', ')); the C runtime must be linked statically (.cargo/config.toml, +crt-static)"
    }
    $unexpected = @($dependents | Where-Object { -not (Test-AllowedDependency $_) })
    if ($unexpected.Count -gt 0) {
        throw "$dll depends on $($unexpected -join ', '), which is not a Windows system DLL on the allow-list; find the crate that pulled it in (cargo tree) instead of extending the list"
    }

    New-Item -ItemType Directory -Force -Path $Out | Out-Null
    # The set is exactly one file: clear out what an earlier assembly left.
    Get-ChildItem -LiteralPath $Out -File | Remove-Item -Force
    Copy-Item -LiteralPath $dll -Destination $Out -Force
    $copied = Get-Item -LiteralPath (Join-Path $Out 'TimeTugEngineC.dll')
    Write-Host ('{0,12:N0}  {1}' -f $copied.Length, $copied.Name)
    Write-Host ('{0,12:N0}  total, 1 file for {1} in {2}' -f $copied.Length, $Rid, (Resolve-Path $Out))
}

try {
    Set-Location (Join-Path $PSScriptRoot '..\..')
    Invoke-RustAssembly
    exit 0
} catch {
    Write-Host "::error::assemble-native.ps1: $($_.Exception.Message)"
    exit 1
}

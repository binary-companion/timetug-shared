<#
.SYNOPSIS
  The C smoke test on Windows: builds the TimeTugEngineC DLL, compiles c-smoke/smoke.c against
  include/timetug_engine.h with the toolchain's clang, and runs it with the DLL on the PATH.

.DESCRIPTION
  The Windows counterpart of c-smoke/run.sh. The Rust library is built (cargo build --release -p timetug-engine-c),
  taken from target\release, and linked through TimeTugEngineC.dll.lib, the import library rustc writes next to the
  DLL. (The plain TimeTugEngineC.lib there is the static library, which must not be linked.) Exits non-zero on any
  failure. -Rust is accepted and changes nothing: it was the switch while the Swift library still existed.
#>
[CmdletBinding()]
param(
    [switch] $Rust
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Checked([string] $Exe, [string[]] $Arguments) {
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe $($Arguments -join ' ') exited with code $LASTEXITCODE" }
}

try {
    Set-Location (Join-Path $PSScriptRoot '..\..')
    Invoke-Checked cargo @('build', '--release', '--locked', '-p', 'timetug-engine-c')
    $targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
    $bin = (Resolve-Path -LiteralPath (Join-Path $targetDir 'release')).Path
    $importName = 'TimeTugEngineC.dll.lib'

    $dll = Join-Path $bin 'TimeTugEngineC.dll'
    if (-not (Test-Path -LiteralPath $dll)) { throw "TimeTugEngineC.dll not found in $bin" }
    $import = Get-ChildItem -LiteralPath $bin -Filter $importName -Recurse | Select-Object -First 1
    if ($null -eq $import) { throw "$importName (the import library) not found under $bin" }

    # The export check, as nm does on macOS and Linux: dumpbin ships with the MSVC tools (vswhere finds them
    # when the shell is not a developer prompt).
    $dumpbin = Get-Command dumpbin -ErrorAction SilentlyContinue
    if ($null -eq $dumpbin) {
        $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
        if (Test-Path -LiteralPath $vswhere) {
            $found = & $vswhere -latest -products * -find 'VC\Tools\MSVC\**\bin\Host*\*\dumpbin.exe' | Select-Object -First 1
            if ($found) { $dumpbin = Get-Command $found }
        }
    }
    if ($null -eq $dumpbin) { throw 'dumpbin not found (it ships with the MSVC build tools)' }
    $listing = & $dumpbin.Source /exports $dll | Out-String
    if ($LASTEXITCODE -ne 0) { throw "dumpbin /exports exited with code $LASTEXITCODE" }
    $names = @('tt_engine_create', 'tt_engine_send', 'tt_engine_reply', 'tt_engine_destroy')
    $missing = @($names | Where-Object { $listing -notmatch "(?m)\s$_(\s|$)" })
    if ($missing.Count -gt 0) { throw "$dll does not export $($missing -join ', ')" }
    Write-Host ('exports: ' + ($names -join ' '))

    $clang = Get-Command clang -ErrorAction SilentlyContinue
    if ($null -eq $clang) { throw 'clang not found on the PATH' }

    $work = Join-Path ([System.IO.Path]::GetTempPath()) ("tt-c-smoke-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $work | Out-Null
    try {
        $exe = Join-Path $work 'smoke.exe'
        Invoke-Checked $clang.Source @('-std=c11', '-Wall', '-Wextra', '-D_CRT_SECURE_NO_WARNINGS', '-I', 'include',
            'c-smoke/smoke.c', $import.FullName, '-o', $exe)
        $env:Path = "$bin;$env:Path"
        Invoke-Checked $exe @((Join-Path $work 'data'))
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }
} catch {
    Write-Host "::error::C smoke test failed: $($_.Exception.Message)"
    exit 1
}

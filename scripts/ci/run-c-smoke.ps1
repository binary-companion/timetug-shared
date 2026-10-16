<#
.SYNOPSIS
  The C smoke test on Windows: builds the TimeTugEngineC DLL, compiles c-smoke/smoke.c against
  include/timetug_engine.h with the toolchain's clang, and runs it with the DLL on the PATH.

.DESCRIPTION
  The Windows counterpart of c-smoke/run.sh. The bin directory comes from
  `swift build --show-bin-path`, never .build\release (no symlink rights on the runners).
  The Swift runtime DLLs the library needs are already on the PATH after
  install-swift-windows.ps1. Exits non-zero on any failure.
#>
[CmdletBinding()]
param([string] $Configuration = 'release')

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Checked([string] $Exe, [string[]] $Arguments) {
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe $($Arguments -join ' ') exited with code $LASTEXITCODE" }
}

try {
    Set-Location (Join-Path $PSScriptRoot '..\..')
    Invoke-Checked swift @('build', '-c', $Configuration, '--product', 'TimeTugEngineC')
    $bin = (& swift build -c $Configuration --show-bin-path | Select-Object -Last 1).Trim()
    if ($LASTEXITCODE -ne 0) { throw "swift build --show-bin-path exited with code $LASTEXITCODE" }

    $dll = Join-Path $bin 'TimeTugEngineC.dll'
    if (-not (Test-Path -LiteralPath $dll)) { throw "TimeTugEngineC.dll not found in $bin" }
    $import = Get-ChildItem -LiteralPath $bin -Filter 'TimeTugEngineC.lib' -Recurse | Select-Object -First 1
    if ($null -eq $import) { throw "TimeTugEngineC.lib (the import library) not found under $bin" }

    $clang = Get-Command clang -ErrorAction SilentlyContinue
    if ($null -eq $clang) { throw 'clang not found on the PATH (it ships with the Swift toolchain)' }

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

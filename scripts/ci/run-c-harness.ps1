<#
.SYNOPSIS
  The C concurrency harness on Windows: builds the TimeTugEngineC DLL, compiles c-harness/harness.c against
  include/timetug_engine.h with clang, and runs it with the DLL on the PATH.

.DESCRIPTION
  The Windows counterpart of c-harness/run.sh. Exactly one of -Swift and -Rust selects the library:
    -Swift  swift build; the bin directory comes from `swift build --show-bin-path`, never .build\release (no symlink
            rights on the runners). The Swift runtime DLLs are on the PATH after install-swift-windows.ps1.
    -Rust   cargo build --release -p timetug-engine-c; the DLL and its import library (TimeTugEngineC.dll.lib, written
            by cargo next to the DLL) are in target\release.
  Extra harness arguments (for example --iterations 20 --case stale_handles) go in -HarnessArguments.
  Exits non-zero on any failure.
#>
[CmdletBinding()]
param(
    [switch] $Swift,
    [switch] $Rust,
    [string] $Configuration = 'release',
    [string[]] $HarnessArguments = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Invoke-Checked([string] $Exe, [string[]] $Arguments) {
    & $Exe @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Exe $($Arguments -join ' ') exited with code $LASTEXITCODE" }
}

try {
    if ($Swift -eq $Rust) { throw 'give exactly one of -Swift and -Rust' }
    Set-Location (Join-Path $PSScriptRoot '..\..')

    if ($Swift) {
        Invoke-Checked swift @('build', '-c', $Configuration, '--product', 'TimeTugEngineC')
        $bin = (& swift build -c $Configuration --show-bin-path | Select-Object -Last 1).Trim()
        if ($LASTEXITCODE -ne 0) { throw "swift build --show-bin-path exited with code $LASTEXITCODE" }
        $importName = 'TimeTugEngineC.lib'
    } else {
        Invoke-Checked cargo @('build', '--release', '--locked', '-p', 'timetug-engine-c')
        $target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path (Get-Location) 'target' }
        $bin = Join-Path $target 'release'
        $importName = 'TimeTugEngineC.dll.lib'
    }

    $dll = Join-Path $bin 'TimeTugEngineC.dll'
    if (-not (Test-Path -LiteralPath $dll)) { throw "TimeTugEngineC.dll not found in $bin" }
    $import = Get-ChildItem -LiteralPath $bin -Filter $importName -Recurse | Select-Object -First 1
    if ($null -eq $import) { throw "$importName (the import library) not found under $bin" }

    $clang = Get-Command clang -ErrorAction SilentlyContinue
    if ($null -eq $clang) {
        # The GitHub runners ship LLVM here; it is not always on the PATH of a pwsh step.
        $candidate = Join-Path ${env:ProgramFiles} 'LLVM\bin\clang.exe'
        if (Test-Path -LiteralPath $candidate) { $clangPath = $candidate } else { throw 'clang not found on the PATH' }
    } else {
        $clangPath = $clang.Source
    }

    $work = Join-Path ([System.IO.Path]::GetTempPath()) ("tt-c-harness-" + [guid]::NewGuid())
    New-Item -ItemType Directory -Path $work | Out-Null
    try {
        $exe = Join-Path $work 'harness.exe'
        Invoke-Checked $clangPath @('-std=c11', '-Wall', '-Wextra', '-Werror', '-O1', '-D_CRT_SECURE_NO_WARNINGS', '-I', 'include',
            'c-harness/harness.c', $import.FullName, '-o', $exe)
        $env:Path = "$bin;$env:Path"
        Invoke-Checked $exe (@((Join-Path $work 'data')) + $HarnessArguments)
    } finally {
        Remove-Item -LiteralPath $work -Recurse -Force -ErrorAction SilentlyContinue
    }
} catch {
    Write-Host "::error::C harness failed: $($_.Exception.Message)"
    exit 1
}

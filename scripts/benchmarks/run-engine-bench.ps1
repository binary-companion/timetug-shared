<#
.SYNOPSIS
  Resource baseline of the engine on Windows (docs/rust-port/baselines.md): assembles the native set, builds
  dotnet/TimeTug.Engine.Bench with it and writes the measurements as JSON.

.DESCRIPTION
  Builds TimeTugEngineC with `swift build -c release` unless -SkipBuild (with -Rust: `cargo build --release --locked
  -p timetug-engine-c --target <triple>`), assembles the DLL with its Swift runtime closure, or alone for -Rust
  (dotnet/pack/assemble-native.ps1) into build/bench/native, builds the harness with that set copied next to
  it, and runs it. Needs a Swift toolchain, Visual Studio's dumpbin and the .NET 8 SDK or newer.

.EXAMPLE
  pwsh scripts/benchmarks/run-engine-bench.ps1 -IdleSeconds 600 -Out build/bench/engine-bench.json
#>
[CmdletBinding()]
param(
    [int] $IdleSeconds = 600,
    [string] $Out = 'build/bench/engine-bench.json',
    [switch] $SkipBuild,
    [switch] $Rust
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Set-Location (Join-Path $PSScriptRoot '..\..')
$rid = if ([System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture -eq 'Arm64') { 'win-arm64' } else { 'win-x64' }
$native = Join-Path 'build/bench/native' $rid

if ($Rust) {
    $triple = if ($rid -eq 'win-arm64') { 'aarch64-pc-windows-msvc' } else { 'x86_64-pc-windows-msvc' }
    if (-not $SkipBuild) {
        cargo build --release --locked -p timetug-engine-c --target $triple
        if ($LASTEXITCODE -ne 0) { throw "cargo build exited with code $LASTEXITCODE" }
    }
    ./dotnet/pack/assemble-native.ps1 -Rust -Rid $rid -Out $native
} else {
    if (-not $SkipBuild) {
        swift build -c release --product TimeTugEngineC
        if ($LASTEXITCODE -ne 0) { throw "swift build exited with code $LASTEXITCODE" }
    }
    ./dotnet/pack/assemble-native.ps1 -Rid $rid -Out $native
}
if ($LASTEXITCODE -ne 0) { throw "assemble-native.ps1 exited with code $LASTEXITCODE" }
$nativeFull = (Resolve-Path -LiteralPath $native).Path

dotnet build dotnet/TimeTug.Engine.Bench -c Release "-p:TimeTugNativeDir=$nativeFull"
if ($LASTEXITCODE -ne 0) { throw "dotnet build exited with code $LASTEXITCODE" }

# The Swift runtime stays off the PATH, as in the tests: the assembled set must be complete.
$env:Path = ($env:Path -split ';' | Where-Object { $_ -and -not (Test-Path -LiteralPath (Join-Path $_ 'swiftCore.dll')) }) -join ';'
Remove-Item Env:TIMETUG_ENGINE_LIBRARY -ErrorAction SilentlyContinue

New-Item -ItemType Directory -Force -Path (Split-Path -Parent (Join-Path (Get-Location) $Out)) | Out-Null
dotnet dotnet/TimeTug.Engine.Bench/bin/Release/net8.0/TimeTug.Engine.Bench.dll --idle-seconds $IdleSeconds --out $Out
if ($LASTEXITCODE -ne 0) { throw "the benchmark exited with code $LASTEXITCODE" }
Write-Host "wrote $Out"

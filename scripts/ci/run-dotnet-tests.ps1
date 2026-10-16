<#
.SYNOPSIS
  Runs the C# tests (dotnet/TimeTug.Engine.Tests) against an assembled native set on Windows and checks the number
  of tests that ran against scripts/ci/dotnet-test-floor.txt.

.DESCRIPTION
  The native set (from dotnet/pack/assemble-native.ps1) is copied next to the test binary. Every PATH entry that
  holds the Swift runtime is removed first, so the tests prove the set is complete: a Swift runtime DLL missing from
  it fails to load here instead of in an app. Exits non-zero when the build or a test fails or too few tests ran.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $NativeDir,
    [string] $Configuration = 'Release',
    [string] $Log = 'dotnet-test.log'
)

Set-StrictMode -Version Latest

Set-Location (Join-Path $PSScriptRoot '..\..')
if (-not (Test-Path -LiteralPath (Join-Path $NativeDir 'TimeTugEngineC.dll'))) {
    Write-Host "::error::TimeTugEngineC.dll not found in $NativeDir (run dotnet/pack/assemble-native.ps1 first)"
    exit 1
}
$native = (Resolve-Path -LiteralPath $NativeDir).Path
$env:Path = ($env:Path -split ';' | Where-Object { $_ -and -not (Test-Path -LiteralPath (Join-Path $_ 'swiftCore.dll')) }) -join ';'
Remove-Item Env:TIMETUG_ENGINE_LIBRARY -ErrorAction SilentlyContinue

# The exit code is read on the very next statement: a later native command would overwrite it.
dotnet test dotnet/TimeTug.Engine.Tests -c $Configuration "-p:TimeTugNativeDir=$native" --logger 'console;verbosity=normal' 2>&1 |
    Tee-Object -FilePath $Log
$testExit = $LASTEXITCODE
if ($testExit -ne 0) {
    Write-Host "::error::dotnet test exited with code $testExit"
    exit $testExit
}
python scripts/ci/count-swift-tests.py $Log --dotnet-floor-file scripts/ci/dotnet-test-floor.txt
exit $LASTEXITCODE

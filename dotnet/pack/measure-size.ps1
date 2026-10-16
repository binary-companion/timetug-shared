<#
.SYNOPSIS
  Prints the size of the engine's native payload per Windows architecture and of the package.

.DESCRIPTION
  For each architecture directory under -NativeRoot (win-x64, win-arm64, as dotnet/pack/assemble-native.ps1 writes
  them): the uncompressed bytes of the native set and the bytes of a Compress-Archive (Optimal) zip of it, a
  stand-in for the MSIX that the Windows app builds; then the size of the .nupkg. Writes a Markdown table to the
  job summary when $GITHUB_STEP_SUMMARY is set. A compressed set above 40 MB (the phase 4 trigger for dropping
  FoundationInternationalization) is reported as a warning, not a failure: the decision is recorded by hand in
  the size note.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $NativeRoot,
    [Parameter(Mandatory)] [string] $Package,
    [long] $TriggerBytes = 40MB
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Format-MB([long] $Bytes) { '{0:N1} MB' -f ($Bytes / 1MB) }

try {
    if (-not (Test-Path -LiteralPath $Package)) { throw "package not found: $Package" }
    $rows = @()
    foreach ($rid in @('win-x64', 'win-arm64')) {
        $dir = Join-Path $NativeRoot $rid
        if (-not (Test-Path -LiteralPath $dir)) { throw "native set not found: $dir" }
        $files = @(Get-ChildItem -LiteralPath $dir -File)
        $raw = ($files | Measure-Object -Property Length -Sum).Sum
        $zip = Join-Path ([System.IO.Path]::GetTempPath()) "tt-native-$rid-$([guid]::NewGuid().ToString('N')).zip"
        try {
            Compress-Archive -Path ($files | ForEach-Object FullName) -DestinationPath $zip -CompressionLevel Optimal
            $zipped = (Get-Item -LiteralPath $zip).Length
        } finally {
            Remove-Item -LiteralPath $zip -Force -ErrorAction SilentlyContinue
        }
        $rows += [pscustomobject]@{ Rid = $rid; Files = $files.Count; Uncompressed = [long]$raw; Zip = [long]$zipped }
        if ($zipped -gt $TriggerBytes) {
            Write-Host "::warning::the compressed native set for $rid is $(Format-MB $zipped), above the $(Format-MB $TriggerBytes) trigger"
        }
    }
    $nupkg = (Get-Item -LiteralPath $Package).Length

    $lines = @('| Architecture | Files | Uncompressed | Zip (Optimal) |', '|---|---:|---:|---:|')
    foreach ($row in $rows) {
        $lines += "| $($row.Rid) | $($row.Files) | $($row.Uncompressed) bytes ($(Format-MB $row.Uncompressed)) | $($row.Zip) bytes ($(Format-MB $row.Zip)) |"
    }
    $lines += ''
    $lines += "Package $(Split-Path -Leaf $Package): $nupkg bytes ($(Format-MB $nupkg))"
    $lines | ForEach-Object { Write-Host $_ }
    if ($env:GITHUB_STEP_SUMMARY) {
        Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value (@('### TimeTug.Engine package size', '') + $lines)
    }
} catch {
    Write-Host "::error::measure-size.ps1: $($_.Exception.Message)"
    exit 1
}

<#
.SYNOPSIS
  Prints the size of the engine's native payload per Windows architecture and of the package.

.DESCRIPTION
  For each architecture directory under -NativeRoot (win-x64, win-arm64, as dotnet/pack/assemble-native.ps1 writes
  them): the uncompressed bytes of the native set and the bytes of a Compress-Archive (Optimal) zip of it, a
  stand-in for the MSIX that the Windows app builds; then the size of the .nupkg. Writes a Markdown table to the
  job summary when $GITHUB_STEP_SUMMARY is set.

  The size gate (phase 3 task 9, design section 6.4): the uncompressed native set of an architecture above
  -TriggerBytes (8 MB = 8,388,608 bytes) is an error and the script exits 1, after the table is written. The zip
  and package sizes are reported only. A failure is handled by the procedure in
  docs/rust-port/size.md ("If the gate fails"); the limit is not raised.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)] [string] $NativeRoot,
    [Parameter(Mandatory)] [string] $Package,
    [long] $TriggerBytes = 8MB
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Format-MB([long] $Bytes) { '{0:N1} MB' -f ($Bytes / 1MB) }

try {
    if (-not (Test-Path -LiteralPath $Package)) { throw "package not found: $Package" }
    $rows = @()
    $over = @()
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
        if ($raw -gt $TriggerBytes) {
            $over += "the uncompressed native set for $rid is $raw bytes ($(Format-MB $raw)), above the $TriggerBytes bytes ($(Format-MB $TriggerBytes)) limit"
        }
    }
    $nupkg = (Get-Item -LiteralPath $Package).Length

    $lines = @('| Architecture | Files | Uncompressed | Zip (Optimal) |', '|---|---:|---:|---:|')
    foreach ($row in $rows) {
        $lines += "| $($row.Rid) | $($row.Files) | $($row.Uncompressed) bytes ($(Format-MB $row.Uncompressed)) | $($row.Zip) bytes ($(Format-MB $row.Zip)) |"
    }
    $lines += ''
    $lines += "Package $(Split-Path -Leaf $Package): $nupkg bytes ($(Format-MB $nupkg))"
    $lines += "Limit: $TriggerBytes bytes ($(Format-MB $TriggerBytes)) uncompressed per architecture; " +
        $(if ($over.Count -eq 0) { 'within it.' } else { 'EXCEEDED.' })
    $lines | ForEach-Object { Write-Host $_ }
    if ($env:GITHUB_STEP_SUMMARY) {
        Add-Content -LiteralPath $env:GITHUB_STEP_SUMMARY -Value (@('### TimeTug.Engine package size', '') + $lines)
    }
    if ($over.Count -gt 0) { throw ($over -join '; ') }
} catch {
    Write-Host "::error::measure-size.ps1: $($_.Exception.Message)"
    exit 1
}

[CmdletBinding()]
param(
  [Parameter()]
  [ValidateRange(1, 100)]
  [int] $MaxFileSizeMiB = 10,

  [Parameter()]
  [ValidateRange(32, 4096)]
  [int] $MaxFilePathLength = 240
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($null -eq (Get-Command git -ErrorAction SilentlyContinue)) {
  throw 'git is unavailable'
}

$repositoryRoot = (& git rev-parse --show-toplevel)
if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($repositoryRoot)) {
  throw 'The current directory is not inside a Git repository'
}

$trackedPaths = @(& git -C $repositoryRoot ls-files)
if ($LASTEXITCODE -ne 0) {
  throw 'Unable to enumerate tracked repository files'
}

$maximumBytes = $MaxFileSizeMiB * 1MB
$violations = [System.Collections.Generic.List[string]]::new()
foreach ($trackedPath in $trackedPaths) {
  if ($trackedPath.Length -gt $MaxFilePathLength) {
    $violations.Add(
      "$trackedPath has $($trackedPath.Length) characters; maximum is $MaxFilePathLength"
    )
  }

  $platformPath = $trackedPath.Replace('/', [IO.Path]::DirectorySeparatorChar)
  $absolutePath = Join-Path $repositoryRoot $platformPath
  if (Test-Path -LiteralPath $absolutePath -PathType Leaf) {
    $fileSize = (Get-Item -LiteralPath $absolutePath -Force).Length
    if ($fileSize -gt $maximumBytes) {
      $violations.Add(
        "$trackedPath is $fileSize bytes; maximum is $maximumBytes bytes"
      )
    }
  }
}

if ($violations.Count -gt 0) {
  throw "Repository push guardrails failed:`n$($violations -join [Environment]::NewLine)"
}

Write-Host (
  "Repository guardrails passed for {0} tracked files (max {1} MiB, {2} characters)." -f `
    $trackedPaths.Count, $MaxFileSizeMiB, $MaxFilePathLength
)

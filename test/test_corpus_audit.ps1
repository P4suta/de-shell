param(
  [Parameter(Mandatory = $true)]
  [string] $AuditScript,

  [Parameter(Mandatory = $true)]
  [string] $DeshellExecutable
)

$ErrorActionPreference = 'Stop'

function Assert-Equal {
  param(
    [Parameter(Mandatory = $true)] $Expected,
    [Parameter(Mandatory = $true)] $Actual,
    [Parameter(Mandatory = $true)][string] $Label
  )

  if ($Expected -ne $Actual) {
    throw "$Label expected '$Expected', found '$Actual'"
  }
}

$temporaryRoot = Join-Path `
  ([IO.Path]::GetTempPath()) `
  ('deshell-corpus-audit-test-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $temporaryRoot

try {
  $staticRoot = Join-Path $temporaryRoot 'static-repository'
  $residualRoot = Join-Path $temporaryRoot 'residual-repository'
  $excludedRoot = Join-Path $temporaryRoot 'excluded-repository'
  $secondExcludedRoot = Join-Path $temporaryRoot 'second-excluded-repository'
  $patternExcludedRoot = Join-Path $temporaryRoot 'pattern-excluded-repository'
  $null = New-Item -ItemType Directory -Path $staticRoot
  $null = New-Item -ItemType Directory -Path $residualRoot
  $null = New-Item -ItemType Directory -Path $excludedRoot
  $null = New-Item -ItemType Directory -Path $secondExcludedRoot
  $null = New-Item -ItemType Directory -Path $patternExcludedRoot

  [IO.File]::WriteAllText(
    (Join-Path $staticRoot 'static.sh'),
    "#!/bin/sh`nset -eu`nprintf audit`n"
  )
  [IO.File]::WriteAllText(
    (Join-Path $residualRoot 'dynamic.sh'),
    "#!/bin/sh`nprintf '%s' `"`$VALUE`"`n"
  )
  [IO.File]::WriteAllText(
    (Join-Path $residualRoot 'Embedded.java'),
    'new ProcessBuilder("sh", "-c", "printf embedded");'
  )
  [IO.File]::WriteAllText(
    (Join-Path $excludedRoot 'skip.sh'),
    "#!/bin/sh`nprintf excluded`n"
  )
  [IO.File]::WriteAllText(
    (Join-Path $secondExcludedRoot 'skip.sh'),
    "#!/bin/sh`nprintf second-excluded`n"
  )
  [IO.File]::WriteAllText(
    (Join-Path $patternExcludedRoot 'skip.sh'),
    "#!/bin/sh`nprintf pattern-excluded`n"
  )

  $powershellExecutable = (Get-Process -Id $PID).Path
  $output = & $powershellExecutable `
    -NoLogo -NoProfile -File $AuditScript `
    -CorpusRoot $temporaryRoot `
    -ExcludeRepository 'excluded-repository,second-excluded-repository' `
    -ExcludePattern 'pattern-*' `
    -DeshellExecutable $DeshellExecutable `
    -Format Json 2>&1
  if ($LASTEXITCODE -ne 0) {
    throw "corpus audit failed: $($output -join ' ')"
  }

  $jsonText = $output -join [Environment]::NewLine
  $report = $jsonText | ConvertFrom-Json
  Assert-Equal 1 $report.schema_version 'schema version'
  Assert-Equal 'shell_files' $report.analysis_scope 'analysis scope'
  Assert-Equal `
    'immediate_children' `
    $report.selection.repository_scope `
    'repository scope'
  Assert-Equal $false $report.selection.source_execution 'source execution'
  Assert-Equal `
    'excluded-repository,second-excluded-repository' `
    (@($report.selection.excluded_repositories) -join ',') `
    'recorded exact exclusions'
  Assert-Equal `
    'pattern-*' `
    (@($report.selection.excluded_patterns) -join ',') `
    'recorded pattern exclusions'
  Assert-Equal 2 $report.summary.repositories_scanned 'repository count'
  Assert-Equal 3 $report.summary.locations.total 'location count'
  Assert-Equal 2 $report.summary.locations.shell_files 'shell-file count'
  Assert-Equal 1 $report.summary.locations.embedded_shell 'embedded count'
  Assert-Equal 0 $report.summary.locations.candidates 'candidate count'
  Assert-Equal 0 $report.summary.analysis_failures 'analysis failures'
  Assert-Equal 1 $report.summary.fully_non_residual 'fully non-residual count'
  $javaInventory = @(
    $report.inventory_groups |
      Where-Object {
        $_.kind -eq 'embedded_shell' -and
        $_.origin -eq 'java' -and
        $_.interpreter -eq 'sh'
      }
  )
  Assert-Equal 1 $javaInventory.Count 'Java inventory group count'
  Assert-Equal 1 $javaInventory[0].count 'Java embedded location count'
  Assert-Equal 2 @($report.files).Count 'file result count'
  Assert-Equal 'residual-repository/dynamic.sh' $report.files[0].location 'sort order'
  Assert-Equal 1 $report.files[0].nodes.residual 'residual node count'
  Assert-Equal 'static-repository/static.sh' $report.files[1].location 'static location'
  Assert-Equal $true $report.files[1].fully_non_residual 'static classification'

  foreach ($file in @($report.files)) {
    if ($null -ne $file.PSObject.Properties['source']) {
      throw "audit JSON must not disclose source for $($file.location)"
    }
  }
  foreach ($root in @(
    $staticRoot,
    $residualRoot,
    $excludedRoot,
    $secondExcludedRoot,
    $patternExcludedRoot
  )) {
    if (Test-Path -LiteralPath (Join-Path $root '.deshell')) {
      throw "audit mutated source repository: $root"
    }
  }

  $secondOutput = & $powershellExecutable `
    -NoLogo -NoProfile -File $AuditScript `
    -CorpusRoot $temporaryRoot `
    -ExcludeRepository 'excluded-repository,second-excluded-repository' `
    -ExcludePattern 'pattern-*' `
    -DeshellExecutable $DeshellExecutable `
    -Format Json 2>&1
  if ($LASTEXITCODE -ne 0) {
    throw "second corpus audit failed: $($secondOutput -join ' ')"
  }
  Assert-Equal `
    $jsonText `
    ($secondOutput -join [Environment]::NewLine) `
    'deterministic JSON report'

  $humanOutput = & $powershellExecutable `
    -NoLogo -NoProfile -File $AuditScript `
    -CorpusRoot $temporaryRoot `
    -ExcludeRepository 'excluded-repository,second-excluded-repository' `
    -ExcludePattern 'pattern-*' `
    -DeshellExecutable $DeshellExecutable `
    -Format Human 2>&1
  if ($LASTEXITCODE -ne 0) {
    throw "human corpus audit failed: $($humanOutput -join ' ')"
  }
  $human = $humanOutput -join [Environment]::NewLine
  foreach ($fragment in @(
    'selection scope=immediate_children',
    'repositories=2',
    'shell_files=2',
    'embedded_shell=1',
    'candidates=0',
    'fully_non_residual=1',
    'nodes formal=',
    'formal file: static-repository/static.sh',
    'residual 1x [sh]'
  )) {
    if (-not $human.Contains($fragment, [StringComparison]::Ordinal)) {
      throw "human report is missing '$fragment': $human"
    }
  }

  $missingExclusionOutput = & $powershellExecutable `
    -NoLogo -NoProfile -File $AuditScript `
    -CorpusRoot $temporaryRoot `
    -ExcludeRepository 'missing-repository' `
    -DeshellExecutable $DeshellExecutable `
    -Format Json 2>&1
  if ($LASTEXITCODE -eq 0) {
    throw 'an unmatched exact exclusion must fail closed'
  }
  $missingExclusionText =
    $missingExclusionOutput -join [Environment]::NewLine
  if (-not $missingExclusionText.Contains(
    "Exact repository exclusion 'missing-repository' did not match",
    [StringComparison]::Ordinal
  )) {
    throw "unmatched exact exclusion diagnostic is unclear: $missingExclusionText"
  }

}
finally {
  $resolved = [IO.Path]::GetFullPath($temporaryRoot)
  $systemTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd(
    [IO.Path]::DirectorySeparatorChar,
    [IO.Path]::AltDirectorySeparatorChar
  ) + [IO.Path]::DirectorySeparatorChar
  $leaf = Split-Path -Leaf $resolved
  if (
    $resolved.StartsWith($systemTemp, [StringComparison]::OrdinalIgnoreCase) -and
    $leaf.StartsWith('deshell-corpus-audit-test-', [StringComparison]::Ordinal)
  ) {
    Remove-Item -LiteralPath $resolved -Recurse -Force
  } else {
    throw "Refusing to remove unverified audit fixture: $resolved"
  }
}

[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)]
  [string] $CorpusRoot,

  [string[]] $ExcludeRepository = @(),

  [string[]] $ExcludePattern = @(),

  [string] $DeshellExecutable = '',

  [ValidateSet('Human', 'Json')]
  [string] $Format = 'Human',

  [string] $OutputPath = ''
)

$ErrorActionPreference = 'Stop'

$unexpectedArguments = @(
  $args | Where-Object { -not [string]::IsNullOrWhiteSpace([string]$_) }
)
if ($unexpectedArguments.Count -ne 0) {
  $unexpected = $unexpectedArguments -join ', '
  throw (
    "Unexpected positional argument(s): $unexpected. When invoking through " +
    'mise, quote comma-separated values passed to -ExcludeRepository or ' +
    '-ExcludePattern.'
  )
}

$ExcludeRepository = @(
  $ExcludeRepository |
    ForEach-Object { $_ -split '[,;]' } |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -ne '' } |
    Sort-Object -CaseSensitive -Unique
)
$ExcludePattern = @(
  $ExcludePattern |
    ForEach-Object { $_ -split '[,;]' } |
    ForEach-Object { $_.Trim() } |
    Where-Object { $_ -ne '' } |
    Sort-Object -CaseSensitive -Unique
)

function Resolve-Executable {
  param([Parameter(Mandatory = $true)][string] $Value)

  if (Test-Path -LiteralPath $Value -PathType Leaf) {
    return (Resolve-Path -LiteralPath $Value).Path
  }
  $command = Get-Command $Value -CommandType Application -ErrorAction Stop
  return $command.Source
}

function Invoke-Deshell {
  param(
    [Parameter(Mandatory = $true)][string] $Executable,
    [Parameter(Mandatory = $true)][string[]] $Arguments
  )

  $output = & $Executable @Arguments 2>&1
  [pscustomobject]@{
    ExitCode = $LASTEXITCODE
    Output = @($output)
    Text = ($output -join [Environment]::NewLine)
  }
}

function Is-ExcludedRepository {
  param(
    [Parameter(Mandatory = $true)][string] $Name,
    [string[]] $ExactNames = @(),
    [string[]] $Patterns = @()
  )

  if ($ExactNames -contains $Name) {
    return $true
  }
  foreach ($pattern in $Patterns) {
    if ($Name -like $pattern) {
      return $true
    }
  }
  return $false
}

function Assert-ContainedPath {
  param(
    [Parameter(Mandatory = $true)][string] $Root,
    [Parameter(Mandatory = $true)][string] $Path,
    [Parameter(Mandatory = $true)][StringComparison] $Comparison
  )

  $resolvedRoot = [IO.Path]::GetFullPath($Root).TrimEnd(
    [char[]]@(
      [IO.Path]::DirectorySeparatorChar,
      [IO.Path]::AltDirectorySeparatorChar
    )
  ) + [IO.Path]::DirectorySeparatorChar
  $resolvedPath = [IO.Path]::GetFullPath($Path)
  if (-not $resolvedPath.StartsWith($resolvedRoot, $Comparison)) {
    throw "scanner returned a path outside '$Root': $Path"
  }
  return $resolvedPath
}

function Get-InventoryOrigin {
  param(
    [Parameter(Mandatory = $true)][string] $Kind,
    [AllowEmptyString()][string] $Locator
  )

  if ($Kind -eq 'shell_file') {
    return 'shell-file'
  }
  if ([string]::IsNullOrWhiteSpace($Locator)) {
    return 'repository-format'
  }
  if ($Locator.StartsWith('source:', [StringComparison]::Ordinal)) {
    $parts = $Locator.Split(':')
    if ($parts.Count -ge 2 -and -not [string]::IsNullOrWhiteSpace($parts[1])) {
      return $parts[1]
    }
  }
  $separator = $Locator.IndexOf(':')
  if ($separator -gt 0) {
    return $Locator.Substring(0, $separator)
  }
  return $Locator
}

$resolvedCorpusRoot = (Resolve-Path -LiteralPath $CorpusRoot).Path
if (-not (Test-Path -LiteralPath $resolvedCorpusRoot -PathType Container)) {
  throw "CorpusRoot is not a directory: $CorpusRoot"
}

if ([string]::IsNullOrWhiteSpace($DeshellExecutable)) {
  $workspaceExecutable = Join-Path `
    (Split-Path -Parent $PSScriptRoot) `
    '_build/default/bin/main.exe'
  if (Test-Path -LiteralPath $workspaceExecutable -PathType Leaf) {
    $DeshellExecutable = $workspaceExecutable
  } else {
    $DeshellExecutable = 'deshell'
  }
}
$deshell = Resolve-Executable $DeshellExecutable
$pathComparison = if ($IsWindows) {
  [StringComparison]::OrdinalIgnoreCase
} else {
  [StringComparison]::Ordinal
}

$repositoryCandidates = @(
  Get-ChildItem -LiteralPath $resolvedCorpusRoot -Directory |
    Sort-Object Name
)
$candidateNames = @($repositoryCandidates | ForEach-Object { $_.Name })
foreach ($exactName in $ExcludeRepository) {
  if ($candidateNames -notcontains $exactName) {
    throw (
      "Exact repository exclusion '$exactName' did not match an immediate " +
      "child of '$CorpusRoot'. When invoking through mise, quote " +
      'comma- or semicolon-separated values.'
    )
  }
}

$repositories = @(
  $repositoryCandidates |
    Where-Object {
      -not (Is-ExcludedRepository `
        -Name $_.Name `
        -ExactNames $ExcludeRepository `
        -Patterns $ExcludePattern)
    }
)

$temporaryRoot = Join-Path `
  ([IO.Path]::GetTempPath()) `
  ('deshell-corpus-audit-' + [Guid]::NewGuid().ToString('N'))
$null = New-Item -ItemType Directory -Path $temporaryRoot

$findings = [Collections.Generic.List[object]]::new()
$shellFiles = [Collections.Generic.List[object]]::new()
$failures = [Collections.Generic.List[string]]::new()
$results = [Collections.Generic.List[object]]::new()

try {
  foreach ($repository in $repositories) {
    $scan = Invoke-Deshell `
      -Executable $deshell `
      -Arguments @('scan', '--root', $repository.FullName, '--format', 'json')
    if ($scan.ExitCode -ne 0) {
      $failures.Add("$($repository.Name): scan failed: $($scan.Text)")
      continue
    }

    try {
      $repositoryFindings = @(($scan.Text | ConvertFrom-Json))
    } catch {
      $failures.Add(
        "$($repository.Name): scan emitted invalid JSON: $($_.Exception.Message)"
      )
      continue
    }

    foreach ($finding in $repositoryFindings) {
      $location = "$($repository.Name)/$([string]$finding.path)"
      $record = [pscustomobject]@{
        Repository = $repository.Name
        Root = $repository.FullName
        Path = [string]$finding.path
        Location = $location
        Kind = [string]$finding.kind
        Interpreter = [string]$finding.interpreter
        Locator = [string]$finding.locator
        ContentHash = [string]$finding.content_hash
      }
      $findings.Add($record)
      if ($record.Kind -eq 'shell_file') {
        $shellFiles.Add($record)
      }
    }
  }

  $caseIndex = 0
  foreach ($file in @($shellFiles | Sort-Object Location)) {
    $caseIndex++
    $caseRoot = Join-Path $temporaryRoot ('case-{0:D6}' -f $caseIndex)
    $null = New-Item -ItemType Directory -Path $caseRoot
    $relativeNative = $file.Path.Replace(
      '/',
      [IO.Path]::DirectorySeparatorChar
    )
    $sourceCandidate = Join-Path $file.Root $relativeNative
    $sourcePath = Assert-ContainedPath `
      -Root $file.Root `
      -Path $sourceCandidate `
      -Comparison $pathComparison

    $actualHash = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).
      Hash.ToLowerInvariant()
    if ($actualHash -ne $file.ContentHash) {
      $message = "$($file.Location): content changed after scan"
      $failures.Add($message)
      $results.Add([ordered]@{
        location = $file.Location
        interpreter = $file.Interpreter
        content_hash = $file.ContentHash
        fully_non_residual = $false
        nodes = [ordered]@{
          formal = 0
          exhaustive = 0
          differential = 0
          residual = 0
        }
        residual_reasons = @()
        error = $message
      })
      continue
    }

    $destination = Join-Path $caseRoot $relativeNative
    $destinationDirectory = Split-Path -Parent $destination
    if (-not (Test-Path -LiteralPath $destinationDirectory -PathType Container)) {
      $null = New-Item -ItemType Directory -Path $destinationDirectory -Force
    }
    Copy-Item -LiteralPath $sourcePath -Destination $destination

    $initialize = Invoke-Deshell `
      -Executable $deshell `
      -Arguments @('init', '--root', $caseRoot)
    if ($initialize.ExitCode -ne 0) {
      $message = "$($file.Location): init failed: $($initialize.Text)"
      $failures.Add($message)
      $results.Add([ordered]@{
        location = $file.Location
        interpreter = $file.Interpreter
        content_hash = $file.ContentHash
        fully_non_residual = $false
        nodes = [ordered]@{
          formal = 0
          exhaustive = 0
          differential = 0
          residual = 0
        }
        residual_reasons = @()
        error = $message
      })
      continue
    }

    $analysis = Invoke-Deshell `
      -Executable $deshell `
      -Arguments @('analyze', '--root', $caseRoot, '--entry', $file.Path)
    if ($analysis.ExitCode -ne 0) {
      $message = "$($file.Location): analyze failed: $($analysis.Text)"
      $failures.Add($message)
      $results.Add([ordered]@{
        location = $file.Location
        interpreter = $file.Interpreter
        content_hash = $file.ContentHash
        fully_non_residual = $false
        nodes = [ordered]@{
          formal = 0
          exhaustive = 0
          differential = 0
          residual = 0
        }
        residual_reasons = @()
        error = $message
      })
      continue
    }

    $evidencePath = Join-Path $caseRoot '.deshell/evidence.json'
    $evidence = Get-Content -LiteralPath $evidencePath -Raw | ConvertFrom-Json
    $levels = @($evidence.nodes | ForEach-Object { [string]$_.guarantee.level })
    $residualReasons = @(
      $evidence.nodes |
        Where-Object { $_.guarantee.level -eq 'residual' } |
        ForEach-Object { [string]$_.guarantee.reason }
    )
    $results.Add([ordered]@{
      location = $file.Location
      interpreter = $file.Interpreter
      content_hash = $file.ContentHash
      fully_non_residual = ($residualReasons.Count -eq 0)
      nodes = [ordered]@{
        formal = @($levels | Where-Object { $_ -eq 'formal' }).Count
        exhaustive = @($levels | Where-Object { $_ -eq 'exhaustive' }).Count
        differential = @($levels | Where-Object { $_ -eq 'differential' }).Count
        residual = $residualReasons.Count
      }
      residual_reasons = $residualReasons
      error = $null
    })
  }
}
finally {
  $resolvedTemporaryRoot = [IO.Path]::GetFullPath($temporaryRoot)
  $systemTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd(
    [char[]]@(
      [IO.Path]::DirectorySeparatorChar,
      [IO.Path]::AltDirectorySeparatorChar
    )
  ) + [IO.Path]::DirectorySeparatorChar
  $leaf = Split-Path -Leaf $resolvedTemporaryRoot
  if (
    $resolvedTemporaryRoot.StartsWith($systemTemp, $pathComparison) -and
    $leaf.StartsWith('deshell-corpus-audit-', [StringComparison]::Ordinal)
  ) {
    Remove-Item -LiteralPath $resolvedTemporaryRoot -Recurse -Force
  } else {
    throw "Refusing to remove unverified audit directory: $resolvedTemporaryRoot"
  }
}

$sortedResults = @($results | Sort-Object { $_.location })
$successfulResults = @($sortedResults | Where-Object { $null -eq $_.error })
$reasonRows = @(
  foreach ($result in $successfulResults) {
    foreach ($reason in @($result.residual_reasons)) {
      [pscustomobject]@{
        Interpreter = $result.interpreter
        Reason = $reason
      }
    }
  }
)
$reasonGroups = @(
  $reasonRows |
    Group-Object Interpreter, Reason |
    ForEach-Object {
      $first = $_.Group[0]
      [ordered]@{
        count = $_.Count
        interpreter = $first.Interpreter
        reason = $first.Reason
      }
    } |
    Sort-Object `
      @{ Expression = { -[int]$_.count } }, `
      @{ Expression = { $_.interpreter } }, `
      @{ Expression = { $_.reason } }
)
$inventoryRows = @(
  $findings | ForEach-Object {
    [pscustomobject]@{
      Kind = $_.Kind
      Origin = Get-InventoryOrigin -Kind $_.Kind -Locator $_.Locator
      Interpreter = if ([string]::IsNullOrWhiteSpace($_.Interpreter)) {
        'unknown'
      } else {
        $_.Interpreter
      }
    }
  }
)
$inventoryGroups = @(
  $inventoryRows |
    Group-Object Kind, Origin, Interpreter |
    ForEach-Object {
      $first = $_.Group[0]
      [ordered]@{
        count = $_.Count
        kind = $first.Kind
        origin = $first.Origin
        interpreter = $first.Interpreter
      }
    } |
    Sort-Object `
      @{ Expression = { $_.kind } }, `
      @{ Expression = { $_.origin } }, `
      @{ Expression = { $_.interpreter } }
)

$formal = ($successfulResults | ForEach-Object { $_.nodes.formal } |
  Measure-Object -Sum).Sum
$exhaustive = ($successfulResults | ForEach-Object { $_.nodes.exhaustive } |
  Measure-Object -Sum).Sum
$differential = ($successfulResults | ForEach-Object { $_.nodes.differential } |
  Measure-Object -Sum).Sum
$residual = ($successfulResults | ForEach-Object { $_.nodes.residual } |
  Measure-Object -Sum).Sum
$fullyNonResidual = @(
  $successfulResults | Where-Object { $_.fully_non_residual }
)

$report = [ordered]@{
  schema_version = 1
  analysis_scope = 'shell_files'
  selection = [ordered]@{
    repository_scope = 'immediate_children'
    excluded_repositories = @($ExcludeRepository)
    excluded_patterns = @($ExcludePattern)
    source_execution = $false
  }
  repositories = @($repositories | ForEach-Object { $_.Name })
  summary = [ordered]@{
    repositories_scanned = $repositories.Count
    locations = [ordered]@{
      total = $findings.Count
      shell_files = @($findings | Where-Object { $_.Kind -eq 'shell_file' }).Count
      embedded_shell = @(
        $findings | Where-Object { $_.Kind -eq 'embedded_shell' }
      ).Count
      candidates = @($findings | Where-Object { $_.Kind -eq 'candidate' }).Count
    }
    analysis_failures = $failures.Count
    fully_non_residual = $fullyNonResidual.Count
    nodes = [ordered]@{
      formal = [int]$formal
      exhaustive = [int]$exhaustive
      differential = [int]$differential
      residual = [int]$residual
    }
  }
  fully_non_residual_files = @(
    $fullyNonResidual | ForEach-Object { $_.location }
  )
  inventory_groups = $inventoryGroups
  residual_reason_groups = $reasonGroups
  files = $sortedResults
  failures = @($failures)
}

$json = $report | ConvertTo-Json -Depth 12
if (-not [string]::IsNullOrWhiteSpace($OutputPath)) {
  $outputFullPath = [IO.Path]::GetFullPath($OutputPath)
  $outputDirectory = Split-Path -Parent $outputFullPath
  if (-not (Test-Path -LiteralPath $outputDirectory -PathType Container)) {
    throw "OutputPath parent does not exist: $outputDirectory"
  }
  [IO.File]::WriteAllText(
    $outputFullPath,
    $json + [Environment]::NewLine,
    [Text.UTF8Encoding]::new($false)
  )
}

if ($Format -eq 'Json') {
  [Console]::Out.WriteLine($json)
} else {
  $selectionLine =
    'selection scope={0} excluded_repositories={1} excluded_patterns={2} source_execution={3}' -f @(
      $report.selection.repository_scope,
      $report.selection.excluded_repositories.Count,
      $report.selection.excluded_patterns.Count,
      $report.selection.source_execution.ToString().ToLowerInvariant()
    )
  [Console]::Out.WriteLine($selectionLine)
  $summaryLine =
    'repositories={0} locations={1} shell_files={2} embedded_shell={3} candidates={4} fully_non_residual={5}' -f @(
      $report.summary.repositories_scanned,
      $report.summary.locations.total,
      $report.summary.locations.shell_files,
      $report.summary.locations.embedded_shell,
      $report.summary.locations.candidates,
      $report.summary.fully_non_residual
    )
  [Console]::Out.WriteLine($summaryLine)
  $nodesLine =
    'nodes formal={0} exhaustive={1} differential={2} residual={3}' -f @(
      $report.summary.nodes.formal,
      $report.summary.nodes.exhaustive,
      $report.summary.nodes.differential,
      $report.summary.nodes.residual
    )
  [Console]::Out.WriteLine($nodesLine)
  foreach ($location in $report.fully_non_residual_files) {
    [Console]::Out.WriteLine("formal file: $location")
  }
  foreach ($group in $report.residual_reason_groups) {
    $reasonLine = 'residual {0}x [{1}] {2}' -f @(
        $group.count,
        $group.interpreter,
        $group.reason
    )
    [Console]::Out.WriteLine($reasonLine)
  }
  foreach ($failure in $report.failures) {
    [Console]::Error.WriteLine("audit failure: $failure")
  }
}

if ($failures.Count -ne 0) {
  exit 1
}

[CmdletBinding()]
param(
  [Parameter()]
  [ValidateSet('Apply', 'Verify')]
  [string] $Mode = 'Verify',

  [Parameter()]
  [ValidatePattern('^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$')]
  [string] $Repository = 'P4suta/de-shell'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repositoryRoot = Split-Path -Parent $PSScriptRoot
$apiVersion = '2026-03-10'
$rulesetFiles = @(
  '.github/rulesets/default-branch.json',
  '.github/rulesets/release-tags.json'
)

function Resolve-RepositoryFile {
  param([Parameter(Mandatory)][string] $RelativePath)

  $absolutePath = Join-Path $repositoryRoot $RelativePath
  if (-not (Test-Path -LiteralPath $absolutePath -PathType Leaf)) {
    throw "Canonical repository file is missing: $RelativePath"
  }
  return $absolutePath
}

function Read-RepositoryJson {
  param([Parameter(Mandatory)][string] $RelativePath)

  return Get-Content -Raw -LiteralPath (Resolve-RepositoryFile $RelativePath) |
    ConvertFrom-Json -Depth 100
}

function Invoke-GitHubRaw {
  param(
    [Parameter(Mandatory)][ValidateSet('GET', 'POST', 'PUT', 'PATCH')][string] $Method,
    [Parameter(Mandatory)][string] $Endpoint,
    [Parameter()][string] $InputFile
  )

  $ghArguments = @(
    'api',
    '--method', $Method,
    '-H', 'Accept: application/vnd.github+json',
    '-H', "X-GitHub-Api-Version: $apiVersion",
    $Endpoint
  )
  if ($InputFile) {
    $ghArguments += @('--input', $InputFile)
  }

  $response = & gh @ghArguments
  if ($LASTEXITCODE -ne 0) {
    throw "GitHub API request failed: $Method $Endpoint"
  }
  return ($response -join [Environment]::NewLine)
}

function Invoke-GitHubJson {
  param(
    [Parameter(Mandatory)][ValidateSet('GET', 'POST', 'PUT', 'PATCH')][string] $Method,
    [Parameter(Mandatory)][string] $Endpoint,
    [Parameter()][string] $InputFile
  )

  $raw = Invoke-GitHubRaw -Method $Method -Endpoint $Endpoint -InputFile $InputFile
  if ([string]::IsNullOrWhiteSpace($raw)) {
    return $null
  }
  return $raw | ConvertFrom-Json -Depth 100
}

function Invoke-GitHubFields {
  param(
    [Parameter(Mandatory)][ValidateSet('POST', 'PATCH')][string] $Method,
    [Parameter(Mandatory)][string] $Endpoint,
    [Parameter(Mandatory)][hashtable] $Fields
  )

  $ghArguments = @(
    'api',
    '--method', $Method,
    '-H', 'Accept: application/vnd.github+json',
    '-H', "X-GitHub-Api-Version: $apiVersion",
    $Endpoint
  )
  foreach ($fieldName in $Fields.Keys) {
    $ghArguments += @('-f', "$fieldName=$($Fields[$fieldName])")
  }
  $null = & gh @ghArguments
  if ($LASTEXITCODE -ne 0) {
    throw "GitHub API request failed: $Method $Endpoint"
  }
}

function Assert-JsonSubset {
  param(
    [Parameter()][AllowNull()] $Expected,
    [Parameter()][AllowNull()] $Actual,
    [Parameter(Mandatory)][string] $Context
  )

  if ($null -eq $Expected) {
    if ($null -ne $Actual) {
      throw "$Context differs: expected null, received $Actual"
    }
    return
  }

  if ($Expected -is [System.Management.Automation.PSCustomObject]) {
    if ($null -eq $Actual) {
      throw "$Context is missing"
    }
    foreach ($expectedProperty in $Expected.PSObject.Properties) {
      $actualProperty = $Actual.PSObject.Properties[$expectedProperty.Name]
      if ($null -eq $actualProperty) {
        throw "$Context.$($expectedProperty.Name) is missing"
      }
      Assert-JsonSubset -Expected $expectedProperty.Value -Actual $actualProperty.Value `
        -Context "$Context.$($expectedProperty.Name)"
    }
    return
  }

  if ($Expected -is [System.Array]) {
    if ($Actual -isnot [System.Array]) {
      throw "$Context differs: expected an array"
    }
    if ($Expected.Count -ne $Actual.Count) {
      throw "$Context differs: expected $($Expected.Count) entries, received $($Actual.Count)"
    }
    for ($index = 0; $index -lt $Expected.Count; $index += 1) {
      Assert-JsonSubset -Expected $Expected[$index] -Actual $Actual[$index] `
        -Context "$Context[$index]"
    }
    return
  }

  if ($Expected -ne $Actual) {
    throw "$Context differs: expected '$Expected', received '$Actual'"
  }
}

function Assert-StringSetEqual {
  param(
    [Parameter(Mandatory)][string[]] $Expected,
    [Parameter(Mandatory)][string[]] $Actual,
    [Parameter(Mandatory)][string] $Context
  )

  $expectedSorted = @($Expected | Sort-Object -CaseSensitive)
  $actualSorted = @($Actual | Sort-Object -CaseSensitive)
  if ($expectedSorted.Count -ne $actualSorted.Count) {
    throw "$Context differs: expected $($expectedSorted.Count) entries, received $($actualSorted.Count)"
  }
  for ($index = 0; $index -lt $expectedSorted.Count; $index += 1) {
    if ($expectedSorted[$index] -cne $actualSorted[$index]) {
      throw "$Context differs: expected '$($expectedSorted -join ', ')', received '$($actualSorted -join ', ')'"
    }
  }
}

function Assert-FeatureFlags {
  $features = Read-RepositoryJson '.github/settings/features.json'
  foreach ($property in $features.PSObject.Properties) {
    if ($property.Value -ne $true) {
      throw "Feature flag $($property.Name) must be true in the canonical policy"
    }
  }
}

function Set-CanonicalLabels {
  $expectedLabels = @(Read-RepositoryJson '.github/settings/labels.json')
  $currentLabels = @(Invoke-GitHubJson -Method GET -Endpoint "repos/$Repository/labels?per_page=100")

  foreach ($expectedLabel in $expectedLabels) {
    $currentLabel = $currentLabels | Where-Object { $_.name -ceq $expectedLabel.name } |
      Select-Object -First 1
    $fields = @{
      name = $expectedLabel.name
      color = $expectedLabel.color
      description = $expectedLabel.description
    }
    if ($null -eq $currentLabel) {
      Invoke-GitHubFields -Method POST -Endpoint "repos/$Repository/labels" -Fields $fields
    }
    else {
      $encodedName = [Uri]::EscapeDataString($currentLabel.name)
      Invoke-GitHubFields -Method PATCH -Endpoint "repos/$Repository/labels/$encodedName" `
        -Fields $fields
    }
  }
}

function Set-CanonicalRulesets {
  $existingRulesets = @(Invoke-GitHubJson -Method GET -Endpoint "repos/$Repository/rulesets")
  foreach ($rulesetFile in $rulesetFiles) {
    $expected = Read-RepositoryJson $rulesetFile
    $existing = $existingRulesets | Where-Object { $_.name -ceq $expected.name } |
      Select-Object -First 1
    $absolutePath = Resolve-RepositoryFile $rulesetFile
    if ($null -eq $existing) {
      $null = Invoke-GitHubJson -Method POST -Endpoint "repos/$Repository/rulesets" `
        -InputFile $absolutePath
    }
    else {
      $null = Invoke-GitHubJson -Method PUT `
        -Endpoint "repos/$Repository/rulesets/$($existing.id)" -InputFile $absolutePath
    }
  }
}

function Test-CanonicalLabels {
  $expectedLabels = @(Read-RepositoryJson '.github/settings/labels.json')
  $currentLabels = @(Invoke-GitHubJson -Method GET -Endpoint "repos/$Repository/labels?per_page=100")
  foreach ($expectedLabel in $expectedLabels) {
    $currentLabel = $currentLabels | Where-Object { $_.name -ceq $expectedLabel.name } |
      Select-Object -First 1
    if ($null -eq $currentLabel) {
      throw "Repository label is missing: $($expectedLabel.name)"
    }
    if ($currentLabel.color -cne $expectedLabel.color) {
      throw "Repository label color differs: $($expectedLabel.name)"
    }
    if ($currentLabel.description -cne $expectedLabel.description) {
      throw "Repository label description differs: $($expectedLabel.name)"
    }
  }
}

function Test-CanonicalRulesets {
  $existingRulesets = @(Invoke-GitHubJson -Method GET -Endpoint "repos/$Repository/rulesets")
  $expectedNames = @()
  foreach ($rulesetFile in $rulesetFiles) {
    $expected = Read-RepositoryJson $rulesetFile
    $expectedNames += $expected.name
    $existing = $existingRulesets | Where-Object { $_.name -ceq $expected.name } |
      Select-Object -First 1
    if ($null -eq $existing) {
      throw "Repository Ruleset is missing: $($expected.name)"
    }
    $actual = Invoke-GitHubJson -Method GET `
      -Endpoint "repos/$Repository/rulesets/$($existing.id)"
    Assert-JsonSubset -Expected $expected -Actual $actual -Context "ruleset.$($expected.name)"
  }

  $unknownRulesets = @($existingRulesets | Where-Object { $expectedNames -cnotcontains $_.name })
  if ($unknownRulesets.Count -gt 0) {
    $unknownNames = ($unknownRulesets | ForEach-Object { $_.name }) -join ', '
    throw "Unmanaged Rulesets require an explicit decision: $unknownNames"
  }
}

if ($null -eq (Get-Command gh -ErrorAction SilentlyContinue)) {
  throw 'gh is unavailable; run this task through mise'
}
$null = & gh auth status --hostname github.com
if ($LASTEXITCODE -ne 0) {
  throw 'gh is not authenticated to github.com'
}

$initialRepository = Invoke-GitHubJson -Method GET -Endpoint "repos/$Repository"
if (-not $initialRepository.permissions.admin) {
  throw "The authenticated account is not an administrator of $Repository"
}
Assert-FeatureFlags

if ($Mode -eq 'Apply') {
  $null = Invoke-GitHubJson -Method PATCH -Endpoint "repos/$Repository" `
    -InputFile (Resolve-RepositoryFile '.github/settings/repository.json')
  $null = Invoke-GitHubJson -Method PUT -Endpoint "repos/$Repository/topics" `
    -InputFile (Resolve-RepositoryFile '.github/settings/topics.json')
  $null = Invoke-GitHubRaw -Method PUT -Endpoint "repos/$Repository/actions/permissions" `
    -InputFile (Resolve-RepositoryFile '.github/settings/actions.json')
  $null = Invoke-GitHubRaw -Method PUT `
    -Endpoint "repos/$Repository/actions/permissions/selected-actions" `
    -InputFile (Resolve-RepositoryFile '.github/settings/selected-actions.json')
  $null = Invoke-GitHubRaw -Method PUT `
    -Endpoint "repos/$Repository/actions/permissions/workflow" `
    -InputFile (Resolve-RepositoryFile '.github/settings/workflow-permissions.json')
  $null = Invoke-GitHubRaw -Method PUT `
    -Endpoint "repos/$Repository/actions/permissions/fork-pr-contributor-approval" `
    -InputFile (Resolve-RepositoryFile '.github/settings/fork-approval.json')

  $null = Invoke-GitHubRaw -Method PUT -Endpoint "repos/$Repository/vulnerability-alerts"
  $null = Invoke-GitHubRaw -Method PUT -Endpoint "repos/$Repository/automated-security-fixes"
  $null = Invoke-GitHubRaw -Method PUT `
    -Endpoint "repos/$Repository/private-vulnerability-reporting"
  $null = Invoke-GitHubRaw -Method PUT -Endpoint "repos/$Repository/immutable-releases"

  Set-CanonicalLabels
  Set-CanonicalRulesets
}

$repositoryState = Invoke-GitHubJson -Method GET -Endpoint "repos/$Repository"
$expectedRepository = Read-RepositoryJson '.github/settings/repository.json'
Assert-JsonSubset -Expected $expectedRepository -Actual $repositoryState -Context 'repository'

$expectedTopics = Read-RepositoryJson '.github/settings/topics.json'
$topicsState = Invoke-GitHubJson -Method GET -Endpoint "repos/$Repository/topics"
Assert-StringSetEqual -Expected $expectedTopics.names -Actual $topicsState.names `
  -Context 'topics.names'

$actionsState = Invoke-GitHubJson -Method GET -Endpoint "repos/$Repository/actions/permissions"
Assert-JsonSubset -Expected (Read-RepositoryJson '.github/settings/actions.json') `
  -Actual $actionsState -Context 'actions'

$selectedActionsState = Invoke-GitHubJson -Method GET `
  -Endpoint "repos/$Repository/actions/permissions/selected-actions"
Assert-JsonSubset -Expected (Read-RepositoryJson '.github/settings/selected-actions.json') `
  -Actual $selectedActionsState -Context 'selected-actions'

$workflowPermissionsState = Invoke-GitHubJson -Method GET `
  -Endpoint "repos/$Repository/actions/permissions/workflow"
Assert-JsonSubset -Expected (Read-RepositoryJson '.github/settings/workflow-permissions.json') `
  -Actual $workflowPermissionsState -Context 'workflow-permissions'

$forkApprovalState = Invoke-GitHubJson -Method GET `
  -Endpoint "repos/$Repository/actions/permissions/fork-pr-contributor-approval"
Assert-JsonSubset -Expected (Read-RepositoryJson '.github/settings/fork-approval.json') `
  -Actual $forkApprovalState -Context 'fork-approval'

$null = Invoke-GitHubRaw -Method GET -Endpoint "repos/$Repository/vulnerability-alerts"
$null = Invoke-GitHubRaw -Method GET -Endpoint "repos/$Repository/automated-security-fixes"
$privateReportingState = Invoke-GitHubJson -Method GET `
  -Endpoint "repos/$Repository/private-vulnerability-reporting"
if (-not $privateReportingState.enabled) {
  throw 'Private vulnerability reporting is disabled'
}
$immutableReleasesState = Invoke-GitHubJson -Method GET `
  -Endpoint "repos/$Repository/immutable-releases"
if (-not $immutableReleasesState.enabled) {
  throw 'Immutable releases are disabled'
}

Test-CanonicalLabels
Test-CanonicalRulesets

$codeownersState = Invoke-GitHubJson -Method GET -Endpoint "repos/$Repository/codeowners/errors"
if (@($codeownersState.errors).Count -gt 0) {
  $messages = @($codeownersState.errors | ForEach-Object { $_.message }) -join '; '
  throw "CODEOWNERS contains errors: $messages"
}

Write-Host "GitHub repository policy is reconciled for $Repository."

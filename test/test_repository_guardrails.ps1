[CmdletBinding()]
param(
  [Parameter(Mandatory)]
  [string] $GuardrailScript
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$guardrail = (Resolve-Path -LiteralPath $GuardrailScript).Path
$temporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$fixtureRoot = Join-Path $temporaryRoot ("deshell-guardrail-{0}" -f [Guid]::NewGuid())

function Assert-GuardrailFailure {
  param(
    [Parameter(Mandatory)][scriptblock] $Action,
    [Parameter(Mandatory)][string] $ExpectedMessage
  )

  try {
    & $Action
  }
  catch {
    if (-not $_.Exception.Message.Contains($ExpectedMessage)) {
      throw "Expected failure containing '$ExpectedMessage', received '$($_.Exception.Message)'"
    }
    return
  }
  throw "Expected guardrail failure containing '$ExpectedMessage'"
}

try {
  $null = New-Item -ItemType Directory -Path $fixtureRoot
  & git init --quiet $fixtureRoot
  if ($LASTEXITCODE -ne 0) {
    throw 'Unable to initialize the guardrail fixture repository'
  }

  Push-Location $fixtureRoot
  try {
    [IO.File]::WriteAllText((Join-Path $fixtureRoot 'ok.txt'), 'ok')
    $hiddenPath = Join-Path $fixtureRoot '.hidden'
    [IO.File]::WriteAllText($hiddenPath, 'hidden')
    [IO.File]::SetAttributes($hiddenPath, [IO.FileAttributes]::Hidden)
    [IO.File]::WriteAllBytes(
      (Join-Path $fixtureRoot 'boundary.bin'),
      [byte[]]::new(1MB)
    )
    & git add -- ok.txt .hidden boundary.bin
    if ($LASTEXITCODE -ne 0) {
      throw 'Unable to stage the passing guardrail fixtures'
    }
    & $guardrail -MaxFileSizeMiB 1 -MaxFilePathLength 32

    [IO.File]::WriteAllBytes(
      (Join-Path $fixtureRoot 'oversized.bin'),
      [byte[]]::new(1MB + 1)
    )
    & git add -- oversized.bin
    if ($LASTEXITCODE -ne 0) {
      throw 'Unable to stage the oversized guardrail fixture'
    }
    Assert-GuardrailFailure -ExpectedMessage 'oversized.bin is' -Action {
      & $guardrail -MaxFileSizeMiB 1 -MaxFilePathLength 32
    }
    & git rm --quiet --force -- oversized.bin
    if ($LASTEXITCODE -ne 0) {
      throw 'Unable to remove the oversized guardrail fixture'
    }

    $longName = ('p' * 29) + '.txt'
    [IO.File]::WriteAllText((Join-Path $fixtureRoot $longName), 'long path')
    & git add -- $longName
    if ($LASTEXITCODE -ne 0) {
      throw 'Unable to stage the long-path guardrail fixture'
    }
    Assert-GuardrailFailure -ExpectedMessage '33 characters; maximum is 32' -Action {
      & $guardrail -MaxFileSizeMiB 1 -MaxFilePathLength 32
    }
  }
  finally {
    Pop-Location
  }
}
finally {
  if (Test-Path -LiteralPath $fixtureRoot) {
    $resolvedFixture = (Resolve-Path -LiteralPath $fixtureRoot).Path
    $temporaryPrefix = $temporaryRoot.TrimEnd(
      [IO.Path]::DirectorySeparatorChar,
      [IO.Path]::AltDirectorySeparatorChar
    ) + [IO.Path]::DirectorySeparatorChar
    if (-not $resolvedFixture.StartsWith(
      $temporaryPrefix,
      [StringComparison]::OrdinalIgnoreCase
    )) {
      throw "Refusing to remove fixture outside the temporary root: $resolvedFixture"
    }
    Remove-Item -LiteralPath $resolvedFixture -Recurse -Force
  }
}

Write-Host 'Repository guardrail contract passed.'

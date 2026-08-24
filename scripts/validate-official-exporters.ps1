[CmdletBinding()]
param(
  [Parameter()]
  [string] $DaggerExecutable = [Environment]::GetEnvironmentVariable(
    'DESHELL_DAGGER_EXE'
  ),

  [Parameter()]
  [string] $DaggerVersion = 'v0.21.8',

  [Parameter()]
  [string] $CwlImage = 'quay.io/commonwl/cwltool@sha256:05e2065d9aa0391e9cb8ed0085a80e419a031ae731b9c6aa52a2c00e554f3e51',

  [Parameter()]
  [ValidateRange(1, 3600)]
  [int] $CommandTimeoutSeconds = 180
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$temporaryBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$validationRoot = [IO.Path]::GetFullPath(
  (Join-Path $temporaryBase ("deshell-official-exporters-{0}" -f [guid]::NewGuid()))
)

function Assert-TemporaryChild {
  param([Parameter(Mandatory)][string] $Path)

  $relative = [IO.Path]::GetRelativePath(
    $temporaryBase,
    [IO.Path]::GetFullPath($Path)
  )
  if (
    [IO.Path]::IsPathRooted($relative) -or
    $relative -eq '..' -or
    $relative.StartsWith(
      "..$([IO.Path]::DirectorySeparatorChar)",
      [StringComparison]::Ordinal
    )
  ) {
    throw "Refusing to use a validation path outside the temporary directory: $Path"
  }
}

function Resolve-Tool {
  param([Parameter(Mandatory)][string] $Name)

  $command = Get-Command $Name -ErrorAction SilentlyContinue
  if ($null -eq $command) {
    throw "$Name is unavailable"
  }
  return $command.Source
}

function Invoke-Checked {
  param(
    [Parameter(Mandatory)][string] $FilePath,
    [Parameter(Mandatory)][string[]] $ArgumentList,
    [Parameter()][string] $WorkingDirectory = $repositoryRoot,
    [Parameter()][switch] $PassThru
  )

  $startInfo = [Diagnostics.ProcessStartInfo]::new()
  $startInfo.FileName = $FilePath
  $startInfo.WorkingDirectory = $WorkingDirectory
  $startInfo.UseShellExecute = $false
  $startInfo.RedirectStandardOutput = $true
  $startInfo.RedirectStandardError = $true
  foreach ($argument in $ArgumentList) {
    $startInfo.ArgumentList.Add($argument)
  }

  $process = [Diagnostics.Process]::new()
  $process.StartInfo = $startInfo
  $timedOut = $false
  try {
    if (-not $process.Start()) {
      throw "Failed to start $FilePath"
    }
    $standardOutput = $process.StandardOutput.ReadToEndAsync()
    $standardError = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit($CommandTimeoutSeconds * 1000)) {
      $timedOut = $true
      $process.Kill($true)
      $process.WaitForExit()
    }
    $outputText = $standardOutput.GetAwaiter().GetResult()
    $diagnosticText = $standardError.GetAwaiter().GetResult()
    $exitCode = $process.ExitCode
  }
  finally {
    $process.Dispose()
  }

  $lines = @(
    if (-not [string]::IsNullOrEmpty($outputText)) {
      $outputText.TrimEnd([char[]]@("`r", "`n")) -split "`r?`n"
    }
  )
  $diagnostics = @(
    if (-not [string]::IsNullOrEmpty($diagnosticText)) {
      $diagnosticText.TrimEnd([char[]]@("`r", "`n")) -split "`r?`n"
    }
  )
  foreach ($line in $lines) {
    Write-Host $line
  }
  foreach ($line in $diagnostics) {
    Write-Host $line
  }
  if ($timedOut) {
    throw "$FilePath timed out after $CommandTimeoutSeconds seconds"
  }
  if ($exitCode -ne 0) {
    throw "$FilePath exited with status $exitCode"
  }
  if ($PassThru) {
    return $lines
  }
}

Assert-TemporaryChild -Path $validationRoot

$opam = Resolve-Tool -Name 'opam'
$docker = Resolve-Tool -Name 'docker'
$git = Resolve-Tool -Name 'git'
if ([string]::IsNullOrWhiteSpace($DaggerExecutable)) {
  $DaggerExecutable = Resolve-Tool -Name 'dagger'
}
else {
  $DaggerExecutable = Resolve-Tool -Name $DaggerExecutable
}

try {
  New-Item -ItemType Directory -Path $validationRoot | Out-Null
  Copy-Item -LiteralPath (Join-Path $repositoryRoot 'examples/static-printf.sh') `
    -Destination (Join-Path $validationRoot 'static-printf.sh')

  $dune = @('exec', '--switch=.', '--', 'dune', 'exec', 'deshell', '--')
  Invoke-Checked -FilePath $opam -ArgumentList (
    $dune + @('init', '--root', $validationRoot)
  )
  Invoke-Checked -FilePath $opam -ArgumentList (
    $dune + @(
      'analyze',
      '--root', $validationRoot,
      '--entry', 'static-printf.sh'
    )
  )
  $cwlArtifact = Invoke-Checked -FilePath $opam -ArgumentList (
    $dune + @('export', '--root', $validationRoot, '--target', 'cwl')
  ) -PassThru
  Set-Content -LiteralPath (Join-Path $validationRoot 'deshell.cwl') `
    -Value ($cwlArtifact -join "`n") -Encoding utf8NoBOM -NoNewline
  $daggerArtifact = Invoke-Checked -FilePath $opam -ArgumentList (
    $dune + @('export', '--root', $validationRoot, '--target', 'dagger')
  ) -PassThru
  Set-Content -LiteralPath (Join-Path $validationRoot 'deshell.dagger.ts') `
    -Value ($daggerArtifact -join "`n") -Encoding utf8NoBOM -NoNewline

  Invoke-Checked -FilePath $docker -ArgumentList @(
    'run',
    '--rm',
    '--entrypoint', 'cwltool',
    '--mount', "type=bind,source=$validationRoot,target=/work,readonly",
    $CwlImage,
    '--no-container',
    '--validate',
    '/work/deshell.cwl'
  )

  $daggerVersionOutput = Invoke-Checked -FilePath $DaggerExecutable `
    -ArgumentList @('version') -PassThru
  if (($daggerVersionOutput -join "`n") -notmatch [regex]::Escape($DaggerVersion)) {
    throw "Expected Dagger $DaggerVersion"
  }

  $daggerModule = Join-Path $validationRoot 'dagger-module'
  New-Item -ItemType Directory -Path $daggerModule | Out-Null
  Invoke-Checked -FilePath $git -ArgumentList @(
    '-C', $daggerModule, 'init', '--quiet'
  )
  Invoke-Checked -FilePath $DaggerExecutable -WorkingDirectory $daggerModule `
    -ArgumentList @('init', '--sdk=typescript', '--name=deshell')
  Copy-Item -LiteralPath (Join-Path $validationRoot 'deshell.dagger.ts') `
    -Destination (Join-Path $daggerModule 'src/index.ts') -Force

  $functions = Invoke-Checked -FilePath $DaggerExecutable `
    -WorkingDirectory $daggerModule -ArgumentList @('functions') -PassThru
  if (($functions -join "`n") -notmatch '(?m)^main(?:\s|$)') {
    throw 'Generated Dagger module does not expose main'
  }

  $result = Invoke-Checked -FilePath $DaggerExecutable `
    -WorkingDirectory $daggerModule -ArgumentList @('call', 'main') -PassThru
  if (($result -join "`n") -notmatch [regex]::Escape('hello from de-shell')) {
    throw 'Generated Dagger module did not produce the expected output'
  }

  Write-Host (
    "Official exporter validation passed with Dagger {0} and {1}." -f `
      $DaggerVersion, $CwlImage
  )
}
finally {
  Assert-TemporaryChild -Path $validationRoot
  if (Test-Path -LiteralPath $validationRoot) {
    Remove-Item -LiteralPath $validationRoot -Recurse -Force
  }
}

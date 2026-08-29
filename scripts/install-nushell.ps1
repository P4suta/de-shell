[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$version = '0.115.1'
$architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString()
$architecturePrefix = switch ($architecture) {
    'X64' { 'x86_64' }
    'Arm64' { 'aarch64' }
    default { throw "Nushell $version is not pinned for architecture $architecture" }
}

if ($IsWindows) {
    $platform = 'pc-windows-msvc'
    $extension = 'zip'
    $executableName = 'nu.exe'
}
elseif ($IsMacOS) {
    $platform = 'apple-darwin'
    $extension = 'tar.gz'
    $executableName = 'nu'
}
elseif ($IsLinux) {
    $platform = 'unknown-linux-gnu'
    $extension = 'tar.gz'
    $executableName = 'nu'
}
else {
    throw "Nushell $version is not pinned for this operating system"
}

$asset = "nu-$version-$architecturePrefix-$platform.$extension"
$expectedDigests = @{
    'nu-0.115.1-x86_64-unknown-linux-gnu.tar.gz' = 'd11d825241f6504a3617c535fa725a9dd6d009c86d7b19fb3168b47635b9d8b0'
    'nu-0.115.1-aarch64-unknown-linux-gnu.tar.gz' = '5c4a5bca0af5b070e903a68fa014cc24e6419d0ac9cec03a2948494b2d310e08'
    'nu-0.115.1-x86_64-apple-darwin.tar.gz' = '0292f4b92af29cfe5d9c4b2ec06eeb325b705d1d6c19536a8bec2b75859b3485'
    'nu-0.115.1-aarch64-apple-darwin.tar.gz' = '2e6ed1eb043869ff05b5f2448a8c443e4d3a93557ba4303b21008a0523c96734'
    'nu-0.115.1-x86_64-pc-windows-msvc.zip' = 'b83009cbc88021f4dc293c49320118886b78363f9a4bb14933d33c8803241f46'
    'nu-0.115.1-aarch64-pc-windows-msvc.zip' = '8f185bc965828208fc9824de32a2e65aa39fa59ebf0a3927dbd0bad1daeb24a1'
}
$expectedDigest = $expectedDigests[$asset]
if ([string]::IsNullOrWhiteSpace($expectedDigest)) {
    throw "Nushell asset is not checksum-pinned: $asset"
}

$temporaryRoot = $env:RUNNER_TEMP
if ([string]::IsNullOrWhiteSpace($temporaryRoot)) {
    $temporaryRoot = [System.IO.Path]::GetTempPath()
}
$archive = Join-Path $temporaryRoot $asset
$installRoot = Join-Path $temporaryRoot "nushell-$version-$architecturePrefix-$platform"
$releaseBase = 'https://github.com/nushell/nushell/releases/download/0.115.1'
$url = "$releaseBase/$asset"

Invoke-WebRequest -Uri $url -OutFile $archive
$actualDigest = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actualDigest -ne $expectedDigest) {
    throw "Nushell archive digest mismatch for ${asset}: expected $expectedDigest, found $actualDigest"
}

New-Item -ItemType Directory -Path $installRoot -Force | Out-Null
if ($IsWindows) {
    Expand-Archive -LiteralPath $archive -DestinationPath $installRoot -Force
}
else {
    & tar --extract --gzip --file $archive --directory $installRoot
    if ($LASTEXITCODE -ne 0) {
        throw "tar failed to extract $asset with exit code $LASTEXITCODE"
    }
}

$nu = Get-ChildItem -LiteralPath $installRoot -Filter $executableName -File -Recurse |
    Select-Object -First 1
if ($null -eq $nu) {
    throw "Nushell archive did not contain $executableName"
}
if (-not $IsWindows) {
    & chmod +x $nu.FullName
    if ($LASTEXITCODE -ne 0) {
        throw "chmod failed for $($nu.FullName) with exit code $LASTEXITCODE"
    }
}

$reportedVersion = (& $nu.FullName --version | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $reportedVersion -ne $version) {
    throw "Nushell version handshake failed: expected $version, found $reportedVersion"
}
$binaryDirectory = Split-Path -Parent $nu.FullName
$env:PATH = "$binaryDirectory$([System.IO.Path]::PathSeparator)$env:PATH"
if (-not [string]::IsNullOrWhiteSpace($env:GITHUB_PATH)) {
    Add-Content -LiteralPath $env:GITHUB_PATH -Value $binaryDirectory -Encoding utf8
}
Write-Output "Installed checksum-pinned Nushell $reportedVersion at $($nu.FullName)"

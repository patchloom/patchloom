$ErrorActionPreference = 'Stop'

# Auto-updated by scripts/update-chocolatey-package.py on each release.
# Do not hand-edit version/url/checksum; run the generator instead.
$packageName = $env:ChocolateyPackageName
$toolsDir = "$(Split-Path -Parent $MyInvocation.MyCommand.Definition)"

$url64 = 'https://github.com/patchloom/patchloom/releases/download/patchloom-v0.14.0/patchloom-x86_64-pc-windows-msvc.zip'
$checksum64 = '6fa31bea8c934840723d02070658e798bc121a5f82ea1f70f5630808a2a9b41d'
$urlArm64 = 'https://github.com/patchloom/patchloom/releases/download/patchloom-v0.14.0/patchloom-aarch64-pc-windows-msvc.zip'
$checksumArm64 = '480f5cb567fc0503d14c8d2b0ef11f134cdd08389d66c8d2ccaf0567174ed064'
$packageArgs = @{
    packageName    = $packageName
    unzipLocation  = $toolsDir
    url64bit       = $url64
    checksum64     = $checksum64
    checksumType64 = 'sha256'
    urlArm64       = $urlArm64
    checksumArm64  = $checksumArm64
    checksumTypeArm64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs

$ErrorActionPreference = 'Stop'

# Auto-updated by scripts/update-chocolatey-package.py on each release.
# Do not hand-edit version/url/checksum; run the generator instead.
$packageName = $env:ChocolateyPackageName
$toolsDir = "$(Split-Path -Parent $MyInvocation.MyCommand.Definition)"

$url64 = 'https://github.com/patchloom/patchloom/releases/download/patchloom-v0.13.0/patchloom-x86_64-pc-windows-msvc.zip'
$checksum64 = 'b194104e93904c82a9ffd30b5a6f9125b1ca41848b24c63353ec4bd775c332f1'
$urlArm64 = 'https://github.com/patchloom/patchloom/releases/download/patchloom-v0.13.0/patchloom-aarch64-pc-windows-msvc.zip'
$checksumArm64 = '1e96356a395d74c86eac9e1ef63617bd2e87f755cb6cd158e97cabc0f1a8dc5d'
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

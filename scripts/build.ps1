param(
    [ValidateSet('debug', 'release')]
    [string]$Profile = 'release',
    [switch]$ReleaseAssets
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..')).Path
Push-Location $projectRoot
try {
    if ($ReleaseAssets -and $Profile -ne 'release') {
        throw '发布产物必须使用 release 配置'
    }
    $metadataJson = & cargo metadata --no-deps --format-version 1 --locked
    if ($LASTEXITCODE -ne 0) { throw "cargo metadata failed: $LASTEXITCODE" }
    $metadata = $metadataJson | ConvertFrom-Json
    $package = @($metadata.packages | Where-Object { $_.name -eq 'cpa-apikey-manager' })[0]
    $version = $package.version

    $buildArguments = @('build', '--locked')
    if ($Profile -eq 'release') { $buildArguments += '--release' }
    & cargo @buildArguments
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed: $LASTEXITCODE" }

    $architecture = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
    if ($IsWindows) {
        $platform = "windows-$architecture"
        $library = 'cpa_apikey_manager.dll'
        $installedName = 'cpa-apikey-manager.dll'
    } elseif ($IsMacOS) {
        $platform = "macos-$architecture"
        $library = 'libcpa_apikey_manager.dylib'
        $installedName = 'cpa-apikey-manager.dylib'
    } else {
        $platform = "linux-$architecture"
        $library = 'libcpa_apikey_manager.so'
        $installedName = 'cpa-apikey-manager.so'
    }
    $packagePath = Join-Path $projectRoot "dist/cpa-apikey-manager-$platform"
    New-Item -ItemType Directory -Path $packagePath -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot "target/$Profile/$library") -Destination (Join-Path $packagePath $installedName) -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'README.md') -Destination $packagePath -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'LICENSE') -Destination $packagePath -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'config.example.yaml') -Destination $packagePath -Force
    $packageDocsPath = Join-Path $packagePath 'docs'
    $packageImagesPath = Join-Path $packageDocsPath 'images'
    New-Item -ItemType Directory -Path $packageImagesPath -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/verification.md') -Destination $packageDocsPath -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/images/management.jpg') -Destination $packageImagesPath -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/images/quota-editor.jpg') -Destination $packageImagesPath -Force
    Copy-Item -LiteralPath (Join-Path $projectRoot 'docs/images/prices.jpg') -Destination $packageImagesPath -Force
    $hash = Get-FileHash -LiteralPath (Join-Path $packagePath $installedName) -Algorithm SHA256
    "$($hash.Hash.ToLowerInvariant())  $installedName" | Set-Content -LiteralPath (Join-Path $packagePath 'SHA256SUMS.txt') -Encoding utf8NoBOM
    $assetName = if ($ReleaseAssets) { "cpa-apikey-manager-$version-$platform" } else { "cpa-apikey-manager-$platform" }
    $archivePath = Join-Path $projectRoot "dist/$assetName.zip"
    Compress-Archive -Path (Join-Path $packagePath '*') -DestinationPath $archivePath -Force
    if ($ReleaseAssets) {
        $extension = [System.IO.Path]::GetExtension($installedName)
        Copy-Item -LiteralPath (Join-Path $packagePath $installedName) -Destination (Join-Path $projectRoot "dist/$assetName$extension") -Force
    }
    Write-Output "安装包已生成：$archivePath"
} finally {
    Pop-Location
}

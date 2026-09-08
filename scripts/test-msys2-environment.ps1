[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$MsysRoot,
    [switch]$IncludeMediaDependencies
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$MsysRoot = (Resolve-Path -LiteralPath $MsysRoot).Path
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-toolchain-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
if ($IncludeMediaDependencies) {
    $media = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-media-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    $inventory.packages = @($inventory.packages) + @($media.packages)
}
$pacmanConfig = Join-Path $PSScriptRoot 'fixtures/msys2-smoke/pacman.conf'
$originalPath = $env:PATH
$originalPkgPath = $env:PKG_CONFIG_PATH
$originalPkgLibdir = $env:PKG_CONFIG_LIBDIR
try {
    $env:PATH = (Join-Path $MsysRoot 'mingw64/bin') + ';' + (Join-Path $MsysRoot 'usr/bin') + ';' + (Join-Path $env:SystemRoot 'System32')
    $env:PKG_CONFIG_PATH = $null
    $env:PKG_CONFIG_LIBDIR = Join-Path $MsysRoot 'mingw64/lib/pkgconfig'
    $installed = @(& (Join-Path $MsysRoot 'usr/bin/pacman.exe') --config $pacmanConfig -Q)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot query the candidate package database.' }
    $expected = @($inventory.packages | ForEach-Object { $_.name + ' ' + $_.version })
    if (@(Compare-Object ($expected | Sort-Object) ($installed | Sort-Object)).Count -ne 0) {
        throw 'Installed packages differ from the pinned initial toolchain.'
    }
    Write-Output "All $($expected.Count) installed package names and versions match."
    & (Join-Path $MsysRoot 'usr/bin/pacman.exe') --config $pacmanConfig -Dk
    if ($LASTEXITCODE -ne 0) { throw 'Candidate package database consistency check failed.' }
    $fileCheck = @(& (Join-Path $MsysRoot 'usr/bin/pacman.exe') --config $pacmanConfig -Qk)
    if ($LASTEXITCODE -ne 0) { throw "Candidate package files are missing: $($fileCheck -join '; ')" }
    Write-Output 'Package database is consistent; no tracked package files are missing.'
    foreach ($tool in @('gcc', 'g++', 'cmake', 'ninja', 'nasm')) {
        & (Join-Path $MsysRoot "mingw64/bin/$tool.exe") --version
        if ($LASTEXITCODE -ne 0) { throw "Candidate tool failed: $tool" }
    }
    $outputDirectory = Join-Path $repositoryRoot ('target/tmp/msys2-environment-' + [guid]::NewGuid().ToString('N'))
    $cmake = Join-Path $MsysRoot 'mingw64/bin/cmake.exe'
    $mediaArguments = @()
    if ($IncludeMediaDependencies) {
        $mediaArguments = @("-DPKG_CONFIG_EXECUTABLE=$MsysRoot/mingw64/bin/pkg-config.exe", '-DTOWAVUE_CHECK_MEDIA=ON')
    }
    & $cmake -S (Join-Path $PSScriptRoot 'fixtures/msys2-smoke') -B $outputDirectory -G Ninja `
        "-DCMAKE_C_COMPILER=$MsysRoot/mingw64/bin/gcc.exe" `
        "-DCMAKE_CXX_COMPILER=$MsysRoot/mingw64/bin/g++.exe" `
        "-DCMAKE_MAKE_PROGRAM=$MsysRoot/mingw64/bin/ninja.exe" -DCMAKE_BUILD_TYPE=Release `
        @mediaArguments
    if ($LASTEXITCODE -ne 0) { throw 'Candidate CMake configuration failed.' }
    & $cmake --build $outputDirectory
    if ($LASTEXITCODE -ne 0) { throw 'Candidate native compile/link failed.' }
    & (Join-Path $outputDirectory 'smoke_test.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Candidate C/C++ DLL runtime smoke test failed.' }
    if ($IncludeMediaDependencies) {
        & (Join-Path $outputDirectory 'media_smoke.exe')
        if ($LASTEXITCODE -ne 0) { throw 'Candidate LV2/VAAPI link/runtime smoke test failed.' }
    }
    Write-Output "Artifacts: $outputDirectory"
    Write-Output 'Initial native toolchain verified; this is not a full FFmpeg build or MSVC ABI test.'
}
finally {
    $env:PATH = $originalPath
    $env:PKG_CONFIG_PATH = $originalPkgPath
    $env:PKG_CONFIG_LIBDIR = $originalPkgLibdir
}

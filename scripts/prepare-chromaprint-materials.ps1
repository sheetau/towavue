[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$PackageArchive,
    [Parameter(Mandatory = $true)][string]$Recipe,
    [Parameter(Mandatory = $true)][string]$SourceArchive,
    [Parameter(Mandatory = $true)][string]$RuntimeDll,
    [Parameter(Mandatory = $true)][string]$LgplLicense,
    [Parameter(Mandatory = $true)][string]$OutputDirectory
)

& (Join-Path $PSScriptRoot 'prepare-native-package-materials.ps1') -Component chromaprint @PSBoundParameters

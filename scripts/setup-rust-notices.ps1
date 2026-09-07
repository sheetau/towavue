[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventory = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs\rust-license-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$noticeDirectory = Join-Path $repositoryRoot 'target\tmp\rust-license-sources'
foreach ($notice in $inventory.retrieved_upstream_notices) {
    $repository = $notice.repository.Replace('https://github.com/', '')
    $relative = $repository + '/' + $notice.commit + '/' + $notice.path
    $destination = Join-Path $noticeDirectory $relative
    if (-not (Test-Path -LiteralPath $destination)) {
        $directory = Split-Path -Parent $destination
        New-Item -ItemType Directory -Path $directory -Force | Out-Null
        $temporary = Join-Path $directory ([IO.Path]::GetRandomFileName())
        try {
            Invoke-WebRequest -UseBasicParsing -Uri ('https://raw.githubusercontent.com/' + $relative) -OutFile $temporary
            if ((Get-FileHash -LiteralPath $temporary -Algorithm SHA256).Hash.ToLowerInvariant() -ne $notice.sha256) {
                throw "Downloaded notice checksum mismatch: $relative"
            }
            Move-Item -LiteralPath $temporary -Destination $destination
        }
        finally {
            if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary }
        }
    }
    if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash.ToLowerInvariant() -ne $notice.sha256) {
        throw "Cached notice checksum mismatch: $relative"
    }
}
Write-Output $noticeDirectory

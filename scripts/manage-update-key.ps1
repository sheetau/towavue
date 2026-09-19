[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Backup','Verify','Restore')][string]$Mode,
    [Parameter(Mandatory)][string]$BackupPath,
    [Security.SecureString]$Password,
    [string]$KeyFile
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-key-backup.ps1')
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$public = Join-Path $repositoryRoot 'packaging/windows/update-public-key.hex'
if (-not $KeyFile) { $KeyFile = Get-TowavueSigningKeyPath }
$BackupPath = Resolve-ReleasePath $BackupPath
$KeyFile = Resolve-ReleasePath $KeyFile
foreach ($path in @($BackupPath,$KeyFile)) {
    if ($path -eq $repositoryRoot -or $path.StartsWith($repositoryRoot + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Keep private keys and encrypted backups outside the repository.' }
}
if ($Mode -eq 'Backup' -and (Test-Path -LiteralPath $BackupPath)) { throw 'Never overwrite an existing signing-key backup.' }
if ($Mode -eq 'Restore' -and (Test-Path -LiteralPath $KeyFile)) { throw 'Never replace an existing release signing key during restore.' }
if ($Mode -ne 'Backup' -and -not (Test-Path -LiteralPath $BackupPath -PathType Leaf)) { throw 'The encrypted signing-key backup is missing.' }
if ($Mode -eq 'Backup' -and -not (Test-Path -LiteralPath $KeyFile -PathType Leaf)) { throw 'The existing signing key is missing; restore its backup instead of generating a replacement identity.' }
$prompted = -not $Password
if ($prompted) { $Password = Read-Host 'Backup password (keep it separately; never enter it in chat)' -AsSecureString }
try {
    switch ($Mode) {
        'Backup' { Export-TowavueKeyBackup $KeyFile $public $BackupPath $Password }
        'Verify' {
            $check = Read-TowavueKeyBackup $BackupPath $Password $public
            try { } finally { $check.rsa.Dispose(); $check.certificate.Dispose() }
        }
        'Restore' { Restore-TowavueKeyBackup $BackupPath $Password $KeyFile $public }
    }
}
finally { if ($prompted) { $Password.Dispose() } }
Write-Output "$Mode succeeded; the key matches the public identity embedded in towavue. No private material was printed."

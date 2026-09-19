[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-key-backup.ps1')
$root = Join-Path (Split-Path -Parent $PSScriptRoot) ('target/tmp/release-key-backup-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
$key = Join-Path $root 'fixture.dpapi'
$public = Join-Path $root 'fixture.hex'
$backup = Join-Path $root 'fixture.pfx'
$restored = Join-Path $root 'restored.dpapi'
$password = ConvertTo-SecureString 'isolated-backup-fixture-password' -AsPlainText -Force
$wrongPassword = ConvertTo-SecureString 'wrong-isolated-fixture-password' -AsPlainText -Force
$shortPassword = ConvertTo-SecureString 'short' -AsPlainText -Force
$beforeStore = @((Get-ChildItem Cert:\CurrentUser\My).Thumbprint | Sort-Object) -join ','
$script:refusals = 0
function Assert-Refused([scriptblock]$Action,[string]$Expected) {
    try { & $Action | Out-Null }
    catch { if (-not $_.Exception.Message.Contains($Expected)) { throw }; $script:refusals++; return }
    throw "Expected refusal: $Expected"
}
try {
    New-TowavueSigningKey $key $public
    $beforeKey = (Get-FileHash -LiteralPath $key).Hash
    $beforePublic = (Get-FileHash -LiteralPath $public).Hash
    Export-TowavueKeyBackup $key $public $backup $password
    $beforeBackup = (Get-FileHash -LiteralPath $backup).Hash
    Restore-TowavueKeyBackup $backup $password $restored $public
    $beforeRestored = (Get-FileHash -LiteralPath $restored).Hash
    $manifest = Join-Path $root 'manifest.txt'
    [IO.File]::WriteAllText($manifest,"towavue-update-v1`n1.0.0`nwindows-x64`n1`n" + ('ab' * 32) + "`n",[Text.UTF8Encoding]::new($false))
    Write-TowavueUpdateSignature $manifest (Join-Path $root 'original.sig') $key $public
    Write-TowavueUpdateSignature $manifest (Join-Path $root 'restored.sig') $restored $public
    if ((Get-FileHash -LiteralPath (Join-Path $root 'original.sig')).Hash -cne (Get-FileHash -LiteralPath (Join-Path $root 'restored.sig')).Hash) { throw 'Restored private key produces a different signature.' }
    Assert-Refused { Export-TowavueKeyBackup $key $public $backup $password } 'Never overwrite'
    Assert-Refused { Restore-TowavueKeyBackup $backup $password $key $public } 'Never replace'
    Assert-Refused { Export-TowavueKeyBackup $key $public (Join-Path $root 'short.pfx') $shortPassword } 'at least 12'
    Assert-Refused { Restore-TowavueKeyBackup $backup $wrongPassword (Join-Path $root 'wrong-password.dpapi') $public } 'could not be decrypted'
    $otherKey = Join-Path $root 'other.dpapi'
    $otherPublic = Join-Path $root 'other.hex'
    New-TowavueSigningKey $otherKey $otherPublic
    Assert-Refused { Restore-TowavueKeyBackup $backup $password (Join-Path $root 'wrong-identity.dpapi') $otherPublic } 'does not match'
    $corrupt = Join-Path $root 'corrupt.pfx'
    [IO.File]::WriteAllBytes($corrupt,[byte[]]@(0,1,2,3))
    Assert-Refused { Restore-TowavueKeyBackup $corrupt $password (Join-Path $root 'corrupt.dpapi') $public } 'could not be decrypted'
    foreach ($name in @('short.pfx','wrong-password.dpapi','wrong-identity.dpapi','corrupt.dpapi')) {
        if (Test-Path -LiteralPath (Join-Path $root $name)) { throw 'Rejected operation published an output.' }
    }
    if (@(Get-ChildItem -LiteralPath $root -Force -Filter '.towavue-key-*').Count) { throw 'Encrypted temporary backup/restore files were retained.' }
    if ($beforeKey -cne (Get-FileHash -LiteralPath $key).Hash -or $beforePublic -cne (Get-FileHash -LiteralPath $public).Hash -or
        $beforeBackup -cne (Get-FileHash -LiteralPath $backup).Hash -or $beforeRestored -cne (Get-FileHash -LiteralPath $restored).Hash) { throw 'Original key/backup files changed.' }
    $afterStore = @((Get-ChildItem Cert:\CurrentUser\My).Thumbprint | Sort-Object) -join ','
    if ($beforeStore -cne $afterStore) { throw 'User certificate store changed.' }
}
finally { $password.Dispose(); $wrongPassword.Dispose(); $shortPassword.Dispose() }
Write-Output "PASS: AES256_SHA256 backup, ephemeral import, exact private-key signature recovery, $script:refusals refusal cases, original preservation, temporary-file cleanup and unchanged user certificate store. Isolated fixture keys only; no production key read or export. Evidence: $root"

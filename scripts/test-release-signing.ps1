[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-signing.ps1')
$root = Join-Path (Split-Path -Parent $PSScriptRoot) ('target/tmp/release-signing-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($root) | Out-Null
$key = Join-Path $root 'private.dpapi'
$public = Join-Path $root 'public.hex'
$manifest = Join-Path $root 'manifest.txt'
$signature = Join-Path $root 'manifest.sig'
New-TowavueSigningKey $key $public
$beforeKey = (Get-FileHash -LiteralPath $key).Hash
$beforePublic = (Get-FileHash -LiteralPath $public).Hash
function Assert-Refused([scriptblock]$Action) {
    $refused = $false
    try { & $Action } catch { $refused = $true }
    if (-not $refused) { throw 'Expected refusal.' }
}
Assert-Refused { New-TowavueSigningKey $key $public }
[IO.File]::WriteAllText($manifest, "towavue-update-v1`n0.0.0`nwindows-x64`n1`n" + ('ab' * 32) + "`n", [Text.UTF8Encoding]::new($false))
Write-TowavueUpdateSignature $manifest $signature $key $public
$rsa = Read-TowavueSigningKey $key $public
try {
    $bytes = [IO.File]::ReadAllBytes($manifest)
    $sig = [IO.File]::ReadAllBytes($signature)
    if ($sig.Length -ne 512 -or -not $rsa.VerifyData($bytes, $sig, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)) { throw 'Signature round trip failed.' }
    $bytes[0] = $bytes[0] -bxor 1
    if ($rsa.VerifyData($bytes, $sig, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)) { throw 'Changed metadata accepted.' }
}
finally { $rsa.Dispose() }
Assert-Refused { Write-TowavueUpdateSignature $manifest $signature $key $public }
$wrong = Join-Path $root 'wrong.hex'
[IO.File]::WriteAllText($wrong, '00', [Text.UTF8Encoding]::new($false))
Assert-Refused { Read-TowavueSigningKey $key $wrong }
if ((Get-FileHash -LiteralPath $key).Hash -ne $beforeKey -or (Get-FileHash -LiteralPath $public).Hash -ne $beforePublic) { throw 'Existing key material changed.' }
Write-Output 'PASS: protected key round trip, signature verification, tamper refusal, mismatched public key and no-overwrite checks. No production key used.'

# Release metadata signatures are separate from Windows Authenticode.
# Private material is protected for the current Windows user and never staged.
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security

function Get-TowavueSigningKeyPath {
    $root = [Environment]::GetFolderPath([Environment+SpecialFolder]::LocalApplicationData)
    if (-not $root) { throw 'LocalApplicationData is unavailable.' }
    Join-Path $root 'towavue-release/update-signing-key.dpapi'
}

function Get-TowavuePublicKeyHex($Rsa) {
    $blob = $Rsa.Key.Export([Security.Cryptography.CngKeyBlobFormat]::GenericPublicBlob)
    ([BitConverter]::ToString($blob)).Replace('-', '').ToLowerInvariant()
}

function Read-TowavueSigningKey([string]$KeyFile, [string]$PublicKeyFile) {
    $protected = [IO.File]::ReadAllBytes($KeyFile)
    $plain = [Security.Cryptography.ProtectedData]::Unprotect($protected, $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
    $rsa = [Security.Cryptography.RSACng]::new()
    try {
        $rsa.FromXmlString([Text.Encoding]::UTF8.GetString($plain))
        $expected = [IO.File]::ReadAllText($PublicKeyFile, [Text.Encoding]::UTF8).Trim()
        if ($rsa.KeySize -ne 4096 -or (Get-TowavuePublicKeyHex $rsa) -cne $expected) {
            throw 'Signing key does not match the public key embedded in the application.'
        }
        return $rsa
    }
    catch { $rsa.Dispose(); throw }
    finally { [Array]::Clear($plain, 0, $plain.Length) }
}

function New-TowavueSigningKey([string]$KeyFile, [string]$PublicKeyFile) {
    if ((Test-Path -LiteralPath $KeyFile) -or (Test-Path -LiteralPath $PublicKeyFile)) {
        throw 'Key initialization requires two new paths; never replace an existing release identity.'
    }
    $rsa = [Security.Cryptography.RSACng]::new(4096)
    try {
        $plain = [Text.Encoding]::UTF8.GetBytes($rsa.ToXmlString($true))
        try {
            $protected = [Security.Cryptography.ProtectedData]::Protect($plain, $null, [Security.Cryptography.DataProtectionScope]::CurrentUser)
        }
        finally { [Array]::Clear($plain, 0, $plain.Length) }
        [IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($KeyFile))) | Out-Null
        $stream = [IO.File]::Open($KeyFile, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.Write($protected, 0, $protected.Length); $stream.Flush($true) }
        finally { $stream.Dispose() }
        $bytes = [Text.Encoding]::UTF8.GetBytes((Get-TowavuePublicKeyHex $rsa) + "`n")
        $stream = [IO.File]::Open($PublicKeyFile, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) }
        finally { $stream.Dispose() }
    }
    finally { $rsa.Dispose() }
    $check = Read-TowavueSigningKey $KeyFile $PublicKeyFile
    $check.Dispose()
}

function Write-TowavueUpdateSignature([string]$Manifest, [string]$Signature, [string]$KeyFile, [string]$PublicKeyFile) {
    $bytes = [IO.File]::ReadAllBytes($Manifest)
    if ($bytes.Length -gt 256) { throw 'Update manifest exceeds its protocol limit.' }
    $rsa = Read-TowavueSigningKey $KeyFile $PublicKeyFile
    try {
        $signatureBytes = $rsa.SignData($bytes, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)
        if (-not $rsa.VerifyData($bytes, $signatureBytes, [Security.Cryptography.HashAlgorithmName]::SHA256, [Security.Cryptography.RSASignaturePadding]::Pkcs1)) { throw 'Signature self-verification failed.' }
        $stream = [IO.File]::Open($Signature, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::None)
        try { $stream.Write($signatureBytes, 0, $signatureBytes.Length); $stream.Flush($true) }
        finally { $stream.Dispose() }
    }
    finally { $rsa.Dispose() }
}

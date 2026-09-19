. (Join-Path $PSScriptRoot 'release-signing.ps1')
. (Join-Path $PSScriptRoot 'release-materials.ps1')

function Read-TowavueKeyBackup([string]$BackupPath, [Security.SecureString]$Password, [string]$PublicKeyFile) {
    $BackupPath = Resolve-ReleasePath $BackupPath
    if (-not $Password -or $Password.Length -lt 1) { throw 'A backup password is required.' }
    $certificate = $null
    $rsa = $null
    try {
        $flags = [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::Exportable -bor [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet
        try { $certificate = [Security.Cryptography.X509Certificates.X509Certificate2]::new($BackupPath,$Password,$flags) }
        catch { throw 'Signing-key backup could not be decrypted or parsed.' }
        if (-not $certificate.HasPrivateKey) { throw 'Signing-key backup has no private key.' }
        $rsa = [Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($certificate)
        if ($rsa -and -not $rsa.Key.IsEphemeral) { throw 'Signing-key backup import must remain ephemeral.' }
        $expected = [IO.File]::ReadAllText((Resolve-ReleasePath $PublicKeyFile),[Text.Encoding]::UTF8).Trim()
        if (-not $rsa -or $rsa.KeySize -ne 4096 -or (Get-TowavuePublicKeyHex $rsa) -cne $expected) { throw 'Backup key does not match the application public key.' }
        $challenge = [Text.Encoding]::UTF8.GetBytes("towavue-key-backup-v1`n")
        $signature = $rsa.SignData($challenge,[Security.Cryptography.HashAlgorithmName]::SHA256,[Security.Cryptography.RSASignaturePadding]::Pkcs1)
        if (-not $rsa.VerifyData($challenge,$signature,[Security.Cryptography.HashAlgorithmName]::SHA256,[Security.Cryptography.RSASignaturePadding]::Pkcs1)) { throw 'Backup private-key proof failed.' }
        # Keep both handles alive until the caller disposes RSA, then certificate.
        # EphemeralKeySet prevents importing any key into the user's key store.
        return [pscustomobject]@{rsa=$rsa;certificate=$certificate}
    }
    catch {
        if ($rsa) { $rsa.Dispose() }
        if ($certificate) { $certificate.Dispose() }
        throw
    }
}

function Export-TowavueKeyBackup([string]$KeyFile, [string]$PublicKeyFile, [string]$BackupPath, [Security.SecureString]$Password) {
    $KeyFile = Resolve-ReleasePath $KeyFile
    $BackupPath = Resolve-ReleasePath $BackupPath
    if (Test-Path -LiteralPath $BackupPath) { throw 'Never overwrite an existing signing-key backup.' }
    $parent = Split-Path -Parent $BackupPath
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) { throw 'Backup parent directory must exist.' }
    if (-not $Password -or $Password.Length -lt 12) { throw 'Use a long unique backup password of at least 12 characters.' }
    $rsa = Read-TowavueSigningKey $KeyFile $PublicKeyFile
    $certificate = $null
    $intermediatePassword = $null
    $random = [byte[]]::new(32)
    $intermediate = Join-Path $parent ('.towavue-key-' + [guid]::NewGuid().ToString('N') + '.input.pfx')
    $partial = Join-Path $parent ('.towavue-key-' + [guid]::NewGuid().ToString('N') + '.partial.pfx')
    try {
        $request = [Security.Cryptography.X509Certificates.CertificateRequest]::new('CN=towavue update signing backup',$rsa,[Security.Cryptography.HashAlgorithmName]::SHA256,[Security.Cryptography.RSASignaturePadding]::Pkcs1)
        $certificate = $request.CreateSelfSigned([DateTimeOffset]::UtcNow.AddDays(-1),[DateTimeOffset]::UtcNow.AddYears(30))
        $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
        try { $rng.GetBytes($random) } finally { $rng.Dispose() }
        $intermediatePassword = ConvertTo-SecureString ([Convert]::ToBase64String($random)) -AsPlainText -Force
        # Windows PKI -Cert omits an ephemeral CNG private key. First use .NET's
        # encrypted PFX container with a fresh random password, then rewrap its
        # bags through Windows PKI's AES256_SHA256 export. No plaintext key file
        # or certificate-store registration is used at either step.
        $encrypted = $certificate.Export([Security.Cryptography.X509Certificates.X509ContentType]::Pfx,$intermediatePassword)
        $stream = [IO.File]::Open($intermediate,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
        try { $stream.Write($encrypted,0,$encrypted.Length); $stream.Flush($true) } finally { $stream.Dispose() }
        $pfxData = Get-PfxData -FilePath $intermediate -Password $intermediatePassword
        Export-PfxCertificate -PFXData $pfxData -FilePath $partial -Password $Password -CryptoAlgorithmOption AES256_SHA256 -NoProperties -NoClobber | Out-Null
        $check = Read-TowavueKeyBackup $partial $Password $PublicKeyFile
        try { } finally { $check.rsa.Dispose(); $check.certificate.Dispose() }
        [IO.File]::Move($partial,$BackupPath)
    }
    finally {
        if ($certificate) { $certificate.Dispose() }
        $rsa.Dispose()
        if ($intermediatePassword) { $intermediatePassword.Dispose() }
        [Array]::Clear($random,0,$random.Length)
        foreach ($path in @($intermediate,$partial)) { if (Test-Path -LiteralPath $path -PathType Leaf) { Remove-Item -LiteralPath $path } }
    }
}

function Restore-TowavueKeyBackup([string]$BackupPath, [Security.SecureString]$Password, [string]$KeyFile, [string]$PublicKeyFile) {
    $KeyFile = Resolve-ReleasePath $KeyFile
    if (Test-Path -LiteralPath $KeyFile) { throw 'Never replace an existing release signing key during restore.' }
    $check = Read-TowavueKeyBackup $BackupPath $Password $PublicKeyFile
    $partial = $null
    try {
        # This policy applies only to the imported ephemeral key in this process.
        # Export is needed in memory for the existing current-user DPAPI format;
        # no persisted Windows key policy or certificate store is changed.
        $policy = [int]($check.rsa.Key.ExportPolicy -bor [Security.Cryptography.CngExportPolicies]::AllowPlaintextExport)
        $check.rsa.Key.SetProperty([Security.Cryptography.CngProperty]::new('Export Policy',[BitConverter]::GetBytes($policy),[Security.Cryptography.CngPropertyOptions]::None))
        $plain = [Text.Encoding]::UTF8.GetBytes($check.rsa.ToXmlString($true))
        try { $protected = [Security.Cryptography.ProtectedData]::Protect($plain,$null,[Security.Cryptography.DataProtectionScope]::CurrentUser) }
        finally { [Array]::Clear($plain,0,$plain.Length) }
        [IO.Directory]::CreateDirectory((Split-Path -Parent $KeyFile)) | Out-Null
        $partial = Join-Path (Split-Path -Parent $KeyFile) ('.towavue-key-' + [guid]::NewGuid().ToString('N') + '.partial.dpapi')
        $stream = [IO.File]::Open($partial,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
        try { $stream.Write($protected,0,$protected.Length); $stream.Flush($true) } finally { $stream.Dispose() }
        $restored = Read-TowavueSigningKey $partial $PublicKeyFile
        $restored.Dispose()
        [IO.File]::Move($partial,$KeyFile)
    }
    finally {
        $check.rsa.Dispose(); $check.certificate.Dispose()
        if ($partial -and (Test-Path -LiteralPath $partial -PathType Leaf)) { Remove-Item -LiteralPath $partial }
    }
}

. (Join-Path $PSScriptRoot 'release-materials.ps1')

function Read-TowavueReleaseArtifacts([string]$Directory, [string]$PublicKeyFile, [string]$Repository = (Split-Path -Parent $PSScriptRoot)) {
    $Directory = Resolve-ReleasePath $Directory
    $receiptPath = Join-Path $Directory 'RELEASE.json'
    $receipt = Get-Content -LiteralPath $receiptPath -Raw -Encoding UTF8 | ConvertFrom-Json
    Assert-ReleaseVersion $receipt.product_version
    if ($receipt.schema_version -ne 1 -or $receipt.repository -cne 'sheetau/towavue' -or
        $receipt.tag -cne ('v' + $receipt.product_version) -or $receipt.source_commit -cnotmatch '^[0-9a-f]{40}\z') { throw 'Invalid release receipt identity.' }
    $expected = @('SHA256SUMS.txt',('towavue-' + $receipt.product_version + '-sources.zip'),('towavue-' + $receipt.product_version + '-windows-x64-setup.exe'),'towavue-update-v1.sig','towavue-update-v1.txt')
    $assetsDirectory = Join-Path $Directory 'assets'
    $tree = Get-ReleaseTree $assetsDirectory
    if ($receipt.assets.Count -ne 5 -or $tree.files.Count -ne 5) { throw 'Release asset coverage differs.' }
    $records = @{}
    foreach ($record in $receipt.assets) {
        if ($record.name -cnotin $expected -or $records.ContainsKey($record.name) -or
            $record.sha256 -cnotmatch '^[0-9a-f]{64}\z' -or $record.bytes -lt 1) { throw 'Invalid release asset record.' }
        Assert-ReleaseFile (Join-Path $assetsDirectory $record.name) $record
        $records.Add($record.name,$record)
    }
    foreach ($record in $tree.files) { if (-not $records.ContainsKey($record.name)) { throw 'Unlisted release asset.' } }
    $setup = $records['towavue-' + $receipt.product_version + '-windows-x64-setup.exe']
    if ($setup.bytes -gt 512MB) { throw 'Setup exceeds the updater size limit.' }
    $manifest = [IO.File]::ReadAllBytes((Join-Path $assetsDirectory 'towavue-update-v1.txt'))
    $expectedManifest = "towavue-update-v1`n$($receipt.product_version)`nwindows-x64`n$($setup.bytes)`n$($setup.sha256)`n"
    if ($manifest.Length -gt 256 -or [Text.Encoding]::UTF8.GetString($manifest) -cne $expectedManifest) { throw 'Release update metadata differs from the payload.' }
    $signature = [IO.File]::ReadAllBytes((Join-Path $assetsDirectory 'towavue-update-v1.sig'))
    if ($signature.Length -ne 512) { throw 'Invalid release signature length.' }
    $hex = [IO.File]::ReadAllText((Resolve-ReleasePath $PublicKeyFile),[Text.Encoding]::UTF8).Trim()
    if ($hex -cnotmatch '^(?:[0-9a-f]{2}){1,2048}\z') { throw 'Invalid release public key.' }
    $blob = [byte[]]::new($hex.Length / 2)
    for ($i = 0; $i -lt $blob.Length; $i++) { $blob[$i] = [Convert]::ToByte($hex.Substring($i * 2,2),16) }
    $key = [Security.Cryptography.CngKey]::Import($blob,[Security.Cryptography.CngKeyBlobFormat]::GenericPublicBlob)
    $rsa = [Security.Cryptography.RSACng]::new($key)
    try {
        if ($rsa.KeySize -ne 4096 -or -not $rsa.VerifyData($manifest,$signature,[Security.Cryptography.HashAlgorithmName]::SHA256,[Security.Cryptography.RSASignaturePadding]::Pkcs1)) { throw 'Release signature verification failed.' }
    }
    finally { $rsa.Dispose(); $key.Dispose() }
    $checksumLines = @($tree.files | Where-Object name -cne 'SHA256SUMS.txt' | ForEach-Object { $_.sha256 + '  ' + $_.name })
    $checksums = [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes((Join-Path $assetsDirectory 'SHA256SUMS.txt')))
    if ($checksums -cne (($checksumLines -join "`n") + "`n")) { throw 'Release checksum list differs.' }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead((Join-Path $assetsDirectory ('towavue-' + $receipt.product_version + '-sources.zip')))
    try {
        $bindingEntries = @($zip.Entries | Where-Object FullName -ceq 'BINDING.json')
        if ($bindingEntries.Count -ne 1 -or $bindingEntries[0].Length -gt 4MB) { throw 'Invalid source companion binding.' }
        $reader = [IO.StreamReader]::new($bindingEntries[0].Open(),[Text.Encoding]::UTF8)
        try { $binding = $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Dispose() }
        $app = @($binding.files | Where-Object name -ceq 'towavue.exe')
        if ($binding.release_version -cne $receipt.product_version -or $binding.application_source_commit -cne $receipt.source_commit -or $binding.files.Count -ne 95 -or
            $app.Count -ne 1 -or $app[0].sha256 -cne $receipt.executable.sha256 -or $app[0].bytes -ne $receipt.executable.bytes -or
            $app[0].source_commit -cne $receipt.source_commit -or $app[0].source -cne ('towavue-' + $receipt.product_version + '-source.zip')) { throw 'Source companion and release receipt differ.' }
        $sourceEntries = @($zip.Entries | Where-Object FullName -ceq $app[0].source)
        if ($sourceEntries.Count -ne 1) { throw 'Missing or duplicate application source archive.' }
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('towavue-release-source-' + [guid]::NewGuid().ToString('N') + '.zip')
        try {
            [IO.Compression.ZipFileExtensions]::ExtractToFile($sourceEntries[0],$temporary,$false)
            [void](Assert-ReleaseSourceArchive $temporary $Repository $receipt.source_commit)
        }
        finally { if (Test-Path -LiteralPath $temporary -PathType Leaf) { Remove-Item -LiteralPath $temporary } }
    }
    finally { $zip.Dispose() }
    return [pscustomobject]@{receipt=$receipt;receipt_record=(Get-ReleaseRecord $receiptPath 'RELEASE.json');assets_directory=$assetsDirectory}
}

function Get-TowavueDraftNotes($Artifacts, [string]$Notes) {
    $marker = '<!-- towavue-release ' + $Artifacts.receipt.source_commit + ' ' + $Artifacts.receipt_record.sha256 + ' -->'
    return [pscustomobject]@{
        pending=($marker + "`n`nUpload in progress. Do not publish this draft yet.`n")
        complete=($marker + "`n`n" + $Notes.Trim() + "`n")
    }
}

function Get-TowavueDraftPlan($Artifacts, $Release, [string]$TagCommit, $Notes) {
    $receipt = $Artifacts.receipt
    if ($TagCommit -and $TagCommit -cne $receipt.source_commit) { throw 'Release tag belongs to a different source commit.' }
    $missing = [Collections.Generic.List[object]]::new()
    $incomplete = [Collections.Generic.List[object]]::new()
    if ($null -eq $Release) {
        foreach ($record in $receipt.assets) { $missing.Add($record) }
        return [pscustomobject]@{create=$true;missing=@($missing);incomplete=@($incomplete)}
    }
    if ($Release.draft -ne $true -or $Release.prerelease -ne $false) { throw 'Only an unpublished stable draft can be uploaded.' }
    if ($TagCommit -cne $receipt.source_commit -or $Release.tag_name -cne $receipt.tag -or
        ($Release.target_commitish -cne $receipt.source_commit -and $Release.target_commitish -cne 'main') -or
        ($Release.body -cne $Notes.pending -and $Release.body -cne $Notes.complete)) { throw 'Existing draft does not belong to this exact release attempt.' }
    $remoteAssets = @{}
    foreach ($asset in $Release.assets) {
        if ($asset.name -cnotin @($receipt.assets.name) -or $remoteAssets.ContainsKey($asset.name)) { throw 'Draft contains unexpected or duplicate assets.' }
        $remoteAssets.Add($asset.name,$asset)
    }
    foreach ($record in $receipt.assets) {
        if (-not $remoteAssets.ContainsKey($record.name)) { $missing.Add($record); continue }
        $asset = $remoteAssets[$record.name]
        # GitHub may leave an empty starter after an interrupted/failed upload.
        # Only that known empty placeholder may be deleted on an owned draft.
        if ($asset.state -ceq 'starter' -and $asset.size -eq 0 -and -not $asset.digest -and [string]$asset.id -cmatch '^[1-9][0-9]*\z') {
            $incomplete.Add($asset); $missing.Add($record); continue
        }
        if ($asset.state -cne 'uploaded' -or $asset.size -ne $record.bytes -or $asset.digest -cne ('sha256:' + $record.sha256)) { throw "Existing draft asset differs; no replacement is allowed: $($record.name)" }
    }
    return [pscustomobject]@{create=$false;missing=@($missing);incomplete=@($incomplete)}
}

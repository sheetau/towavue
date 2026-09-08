# File-only transaction primitive. Setup must persist the returned journal digest
# before Apply and coordinate registration/recovery; this is not connected yet.
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'setup-update-paths.ps1')
if (-not ('TowavueUpdateFiles' -as [type])) { Add-Type -Path (Join-Path $PSScriptRoot 'setup-update-native.cs') }

function Get-UpdateStreamRecord($Stream) {
    $Stream.Position = 0
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $hash = [BitConverter]::ToString($sha.ComputeHash($Stream)).Replace('-','').ToLowerInvariant() }
    finally { $sha.Dispose(); $Stream.Position = 0 }
    return [pscustomobject]@{bytes=$Stream.Length;sha256=$hash}
}
function Test-UpdateRecord($Actual,$Expected) {
    if (-not $Actual -or -not $Expected) { return (-not $Actual -and -not $Expected) }
    return $Actual.bytes -eq $Expected.bytes -and $Actual.sha256 -ceq $Expected.sha256
}
function Copy-UpdateFile([string]$Source,[string]$Destination,$Expected) {
    Assert-LocalPath $Source
    Assert-LocalPath $Destination
    [TowavueUpdateFiles]::Copy($Source,$Destination)
    $stream = [IO.File]::Open($Destination,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
    try {
        if (-not (Test-UpdateRecord (Get-UpdateStreamRecord $stream) $Expected)) { throw 'Update copy differs from expected bytes.' }
        $stream.Flush($true)
    } finally { $stream.Dispose() }
}
function New-TowavueUpdateTransaction {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$InstallDirectory,
        [Parameter(Mandatory)][string]$IncomingPayloadDirectory,
        [Parameter(Mandatory)][string]$IncomingOwnershipId,
        [Parameter(Mandatory)][string]$NewUninstaller
    )
    $plan = & (Join-Path $PSScriptRoot 'get-setup-update-plan.ps1') -InstallDirectory $InstallDirectory -IncomingPayloadDirectory $IncomingPayloadDirectory -IncomingOwnershipId $IncomingOwnershipId | ConvertFrom-Json
    $NewUninstaller = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($NewUninstaller)
    $uninstaller = Get-Record (Split-Path -Parent $NewUninstaller) (Split-Path -Leaf $NewUninstaller)
    $uninstaller.name = 'Uninstall.exe'
    $directory = Join-Path (Split-Path -Parent $plan.install_directory) ('.towavue-update-' + [guid]::NewGuid().ToString('N'))
    Assert-LocalPath $directory
    New-Item -ItemType Directory -Path $directory | Out-Null
    $markerPath = Join-Path $directory 'new-marker.ini'
    [IO.File]::WriteAllText($markerPath,"[installation]`r`nid=$($plan.incoming_ownership_id)`r`ndirectory=$($plan.install_directory)`r`n",[Text.Encoding]::Unicode)
    $marker = Get-Record $directory 'new-marker.ini'
    $marker.name = 'towavue-install.ini'
    $entries = @($plan.actions) + @(
        [pscustomobject]@{name=$marker.name;action='replace';before=$plan.old_metadata[0];after=$marker},
        [pscustomobject]@{name=$uninstaller.name;action='replace';before=$plan.old_metadata[1];after=$uninstaller}
    )
    # Keep the old executable locked until every other file has its target state.
    $entries = @($entries | Where-Object { $_.name -ine 'towavue.exe' }) + @($entries | Where-Object { $_.name -ieq 'towavue.exe' })
    for ($index = 0; $index -lt $entries.Count; $index++) {
        $entry = $entries[$index]
        if ($entry.action -eq 'keep' -and $entry.name -ine 'towavue.exe') { continue }
        if ($entry.before) {
            Copy-UpdateFile (Join-Path $plan.install_directory $entry.name) (Join-Path $directory "$index.old") $entry.before
        }
        if ($entry.after) {
            $source = switch ($entry.name) {
                'towavue-install.ini' { $markerPath }
                'Uninstall.exe' { $NewUninstaller }
                default { Join-Path $plan.incoming_directory $entry.name }
            }
            Copy-UpdateFile $source (Join-Path $directory "$index.new") $entry.after
        }
    }
    # Immutable journal, published only after every backup and staged file. The
    # independently retained digest detects corruption, not a hostile same user.
    $journal = [ordered]@{schema_version=1;directory=$directory;plan=$plan;entries=$entries}
    $journalPath = Join-Path $directory 'journal.json'
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($journal | ConvertTo-Json -Depth 12))
    $stream = [IO.File]::Open($journalPath,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try { $stream.Write($bytes,0,$bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    return [pscustomobject]@{TransactionDirectory=$directory;JournalSha256=(Get-FileHash -LiteralPath $journalPath).Hash.ToLowerInvariant()}
}

function Invoke-TowavueUpdateTransaction {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][ValidateSet('Apply','Rollback')][string]$Mode,
        [Parameter(Mandatory)][string]$TransactionDirectory,
        [Parameter(Mandatory)][ValidatePattern('^[0-9a-f]{64}$')][string]$JournalSha256
    )
    $directory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($TransactionDirectory).TrimEnd('\','/')
    Assert-LocalPath $directory
    $directory = [TowavueUpdatePaths]::Expand($directory)
    if ((Split-Path -Leaf $directory) -cnotmatch '^\.towavue-update-[0-9a-f]{32}$') { throw 'Invalid update transaction directory.' }
    $held = [Collections.Generic.List[IDisposable]]::new()
    try {
        $journalPath = Join-Path $directory 'journal.json'
        Assert-LocalPath $journalPath
        $journalStream = [IO.File]::Open($journalPath,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
        $held.Add($journalStream)
        if ((Get-UpdateStreamRecord $journalStream).sha256 -cne $JournalSha256) { throw 'Update journal identity differs.' }
        $reader = [IO.StreamReader]::new($journalStream,[Text.Encoding]::UTF8,$true,1024,$true)
        try { $journal = $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Dispose() }
        $install = $journal.plan.install_directory
        Assert-LocalPath $install
        if ($journal.schema_version -ne 1 -or $journal.directory -cne $directory -or
            (Split-Path -Parent $install) -ine (Split-Path -Parent $directory) -or
            [TowavueUpdatePaths]::Expand($install) -ine $install -or $install -ieq $directory -or
            -not $journal.entries -or $journal.entries[-1].name -ine 'towavue.exe') { throw 'Update journal location or schema differs.' }
        if ($Mode -eq 'Apply') {
            $plan = & (Join-Path $PSScriptRoot 'get-setup-update-plan.ps1') -InstallDirectory $install -IncomingPayloadDirectory $journal.plan.incoming_directory -IncomingOwnershipId $journal.plan.incoming_ownership_id | ConvertFrom-Json
            if (($plan | ConvertTo-Json -Depth 10 -Compress) -cne ($journal.plan | ConvertTo-Json -Depth 10 -Compress)) { throw 'Installed or incoming update snapshot changed.' }
        }
        $current = @{}
        $sources = @{}
        $restorePaths = @{}
        $names = @{}
        for ($index = 0; $index -lt $journal.entries.Count; $index++) {
            $entry = $journal.entries[$index]
            if ($entry.name -notin @('Uninstall.exe','towavue-install.ini','licenses/INSTALLED-FILES.json')) { Assert-Name $entry.name }
            if ($names.ContainsKey($entry.name)) { throw 'Duplicate update journal target.' }
            $names.Add($entry.name,$true)
            foreach ($side in @('old','new')) {
                if ($entry.action -eq 'keep' -and $entry.name -ine 'towavue.exe') { break }
                $expected = if ($side -eq 'old') { $entry.before } else { $entry.after }
                if (-not $expected) { continue }
                $path = Join-Path $directory "$index.$side"
                Assert-LocalPath $path
                $stream = [IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
                $held.Add($stream)
                if (-not (Test-UpdateRecord (Get-UpdateStreamRecord $stream) $expected)) { throw "Update recovery source differs: $index.$side" }
                $sources["$index.$side"] = $stream
            }
            $receiptPath = Join-Path $directory "$index.published"
            Assert-LocalPath $receiptPath
            $published = Test-Path -LiteralPath $receiptPath
            if ($published) {
                if ($Mode -eq 'Apply') { throw 'Update transaction was already used; prepare a new one.' }
                $stream = [IO.File]::Open($receiptPath,[IO.FileMode]::Open,[IO.FileAccess]::Read,[IO.FileShare]::Read)
                $held.Add($stream)
                $reader = [IO.StreamReader]::new($stream,[Text.Encoding]::ASCII,$false,1024,$true)
                try { if (-not $entry.after -or $reader.ReadToEnd() -cne $entry.after.sha256) { throw 'Invalid update publication receipt.' } }
                finally { $reader.Dispose() }
            }
            $retired = $false
            $rollbackRetired = $false
            foreach ($phase in @('apply','rollback')) {
                $path = Join-Path $directory "$index.$phase-retired"
                Assert-LocalPath $path
                if (-not (Test-Path -LiteralPath $path)) { continue }
                if ($Mode -eq 'Apply') { throw 'Update transaction was already used; prepare a new one.' }
                $stream = [IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::Delete)
                $held.Add($stream)
                $expected = if ($phase -eq 'apply') { $entry.before } else { $entry.after }
                $record = Get-UpdateStreamRecord $stream
                $oldApp = $phase -eq 'rollback' -and $entry.name -ieq 'towavue.exe' -and (Test-UpdateRecord $record $entry.before)
                if (-not (Test-UpdateRecord $record $expected) -and -not $oldApp) { throw 'Retired update file differs; preserve it.' }
                $retired = $true
                if ($phase -eq 'apply') { $restorePaths[$index] = $path }
                if ($phase -eq 'rollback') { $rollbackRetired = $true }
            }
            $path = Join-Path $install $entry.name
            Assert-LocalPath $path
            $actual = $null
            if (Test-Path -LiteralPath $path) {
                if ([TowavueUpdatePaths]::Expand($path) -ine $path) { throw 'Update target uses a short-name alias.' }
                # Deny new readers/writers, including executable image opens,
                # while permitting our same-volume namespace replacement.
                $stream = [IO.File]::Open($path,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::Delete)
                $held.Add($stream)
                $actual = Get-UpdateStreamRecord $stream
            }
            $old = Test-UpdateRecord $actual $entry.before
            $new = Test-UpdateRecord $actual $entry.after
            $recoverableGap = $Mode -eq 'Rollback' -and -not $actual -and $retired -and (-not $published -or $rollbackRetired)
            if (-not $old -and ($Mode -eq 'Apply' -or (-not $new -and -not $recoverableGap)) -or
                ($Mode -eq 'Rollback' -and -not $actual -and $entry.before -and -not $retired)) { throw "Update target has an unknown state; preserve it: $($entry.name)" }
            if ($Mode -eq 'Rollback' -and -not $old -and $actual -and (Test-Path -LiteralPath (Join-Path $directory "$index.rollback-retired"))) { throw 'Rollback retirement is already occupied; preserve the target.' }
            if ($Mode -eq 'Rollback' -and -not $old -and $entry.before -and
                (-not $restorePaths[$index] -or -not (Test-Path -LiteralPath (Split-Path -Parent $path) -PathType Container))) { throw 'Original retired file or its parent is unavailable; preserve the installation.' }
            $current[$index] = $actual
        }
        # No installed-file mutation occurs before all source and target checks.
        $needsChange = $false
        for ($index = 0; $index -lt $journal.entries.Count; $index++) {
            $expected = if ($Mode -eq 'Apply') { $journal.entries[$index].after } else { $journal.entries[$index].before }
            if (-not (Test-UpdateRecord $current[$index] $expected)) { $needsChange = $true; break }
        }
        $appIndex = $journal.entries.Count - 1
        if ($needsChange -and $current[$appIndex]) {
            if ($Mode -eq 'Rollback' -and -not $restorePaths[$appIndex]) { throw 'Original retired executable is unavailable; preserve the installation.' }
            # A process crash releases locks. Hide the executable before touching
            # DLLs so the interrupted installation cannot launch a mixed payload.
            $appPath = Join-Path $install $journal.entries[$appIndex].name
            $retirement = Join-Path $directory ("$appIndex." + $Mode.ToLowerInvariant() + '-retired')
            Assert-LocalPath $appPath
            Assert-LocalPath $retirement
            if (Test-Path -LiteralPath $retirement) { throw 'Executable retirement is already occupied; preserve the target.' }
            [TowavueUpdateFiles]::Move($appPath,$retirement)
            $current[$appIndex] = $null
        }
        for ($index = 0; $index -lt $journal.entries.Count; $index++) {
            $entry = $journal.entries[$index]
            $expected = if ($Mode -eq 'Apply') { $entry.after } else { $entry.before }
            if (Test-UpdateRecord $current[$index] $expected) { continue }
            $path = Join-Path $install $entry.name
            Assert-LocalPath $path
            $work = Join-Path $directory ('work-' + [guid]::NewGuid().ToString('N'))
            Write-Verbose "$Mode update file: $($entry.name)"
            if ($expected -and $Mode -eq 'Rollback') {
                # Restore the original object, including its hard links and
                # metadata, without allocating another file's worth of space.
                $work = $restorePaths[$index]
            } elseif ($expected) {
                # Preserve the old file's copied security/streams on replacement;
                # truncate only a private independent copy, never the target.
                $copySide = if ($entry.before) { 'old' } else { 'new' }
                [TowavueUpdateFiles]::Copy((Join-Path $directory "$index.$copySide"),$work)
                $stream = [IO.File]::Open($work,[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
                try {
                    if ($copySide -eq 'old') {
                        $stream.SetLength(0)
                        $sources["$index.new"].Position = 0
                        $sources["$index.new"].CopyTo($stream)
                    }
                    if (-not (Test-UpdateRecord (Get-UpdateStreamRecord $stream) $expected)) { throw 'Update work file differs.' }
                    $stream.Flush($true)
                } finally { $stream.Dispose() }
                $parent = Split-Path -Parent $path
                Assert-LocalPath $parent
                [IO.Directory]::CreateDirectory($parent) | Out-Null
            }
            # Legacy Windows rename cannot replace an open target. Retire its
            # name first while retaining its handle. Recovery accepts the gap
            # only when this exact retired file still has recorded bytes.
            if ($current[$index]) {
                $retirement = Join-Path $directory ("$index." + $Mode.ToLowerInvariant() + '-retired')
                Assert-LocalPath $retirement
                [TowavueUpdateFiles]::Move($path,$retirement)
            }
            if ($expected) {
                [TowavueUpdateFiles]::Move($work,$path)
                if ($Mode -eq 'Apply') {
                    $receiptWork = Join-Path $directory ('receipt-' + [guid]::NewGuid().ToString('N'))
                    $stream = [IO.File]::Open($receiptWork,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
                    try {
                        $bytes = [Text.Encoding]::ASCII.GetBytes($expected.sha256)
                        $stream.Write($bytes,0,$bytes.Length)
                        $stream.Flush($true)
                    } finally { $stream.Dispose() }
                    [IO.File]::Move($receiptWork,(Join-Path $directory "$index.published"))
                }
            }
        }
        return [pscustomobject]@{state=$(if ($Mode -eq 'Apply') { 'payload_applied_registration_pending' } else { 'payload_rolled_back' });transaction_directory=$directory;retained=$true}
    } finally {
        foreach ($handle in $held) { $handle.Dispose() }
    }
}

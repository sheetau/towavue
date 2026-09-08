[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$InstallDirectory,
    [Parameter(Mandatory)][string]$IncomingPayloadDirectory,
    [Parameter(Mandatory)][ValidatePattern('^towavue-local-[0-9a-f]{64}$')][string]$IncomingOwnershipId
)

$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'setup-update-paths.ps1')
foreach ($name in @('InstallDirectory','IncomingPayloadDirectory')) {
    $path = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath((Get-Variable -Name $name -ValueOnly)).TrimEnd('\','/')
    Assert-LocalPath $path
    if (-not (Test-Path -LiteralPath $path -PathType Container)) { throw 'Both update directories must exist.' }
    $path = [TowavueUpdatePaths]::Expand($path)
    Set-Variable -Name $name -Value $path
}
if ($InstallDirectory -eq $IncomingPayloadDirectory -or
    $InstallDirectory.StartsWith($IncomingPayloadDirectory + '\',[StringComparison]::OrdinalIgnoreCase) -or
    $IncomingPayloadDirectory.StartsWith($InstallDirectory + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Installed and incoming payload directories must not overlap.' }

function Read-Inventory([string]$Root,[string]$OwnershipId) {
    $record = Get-Record $Root 'licenses/INSTALLED-FILES.json'
    if ('towavue-local-' + $record.sha256 -cne $OwnershipId) { throw 'Update inventory does not match its ownership identity.' }
    $inventory = Get-Content -LiteralPath (Join-Path $Root $record.name) -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($inventory.schema_version -ne 1 -or -not $inventory.files -or $inventory.files -isnot [array]) { throw 'Invalid update inventory schema.' }
    $files = @{}
    foreach ($file in $inventory.files) {
        Assert-Name $file.name
        if ($file.bytes -isnot [int] -and $file.bytes -isnot [long]) { throw 'Invalid update file size.' }
        if ($file.bytes -lt 0 -or $file.sha256 -isnot [string] -or $file.sha256 -cnotmatch '^[0-9a-f]{64}$') { throw 'Invalid update file identity.' }
        if ($files.ContainsKey($file.name)) { throw 'Duplicate update inventory name.' }
        $files.Add($file.name,$file)
    }
    if (-not $files.ContainsKey('towavue.exe')) { throw 'Update inventory has no application executable.' }
    foreach ($name in $files.Keys) {
        $parent = $name
        while ($parent.Contains('/')) {
            $parent = $parent.Substring(0,$parent.LastIndexOf('/'))
            if ($files.ContainsKey($parent)) { throw 'Update inventory contains a file/directory collision.' }
        }
        $actual = Get-Record $Root $name
        if ($actual.bytes -ne $files[$name].bytes -or $actual.sha256 -cne $files[$name].sha256) { throw "Update file differs from its recorded bytes; preserve the replacement: $name" }
    }
    $files.Add($record.name,$record)
    return $files
}

$marker = Get-Record $InstallDirectory 'towavue-install.ini'
if ($marker.bytes -gt 65536) { throw 'Invalid installation marker size.' }
$markerBytes = [IO.File]::ReadAllBytes((Join-Path $InstallDirectory $marker.name))
if ($markerBytes.Length -lt 2 -or $markerBytes[0] -ne 255 -or $markerBytes[1] -ne 254) { throw 'Installation marker must be UTF-16LE with BOM.' }
$values = @{}
$inSection = $false
foreach ($line in ([Text.Encoding]::Unicode.GetString($markerBytes,2,$markerBytes.Length - 2) -split '\r?\n')) {
    if (-not $line) { continue }
    if ($line -eq '[installation]' -and -not $inSection) { $inSection = $true; continue }
    if (-not $inSection -or $line -notmatch '^(id|directory)=(.*)$' -or $values.ContainsKey($Matches[1])) { throw 'Ambiguous installation marker.' }
    $values.Add($Matches[1],$Matches[2])
}
$sameDirectory = $values.directory -eq $InstallDirectory
if (-not $sameDirectory -and $values.directory -match '^[A-Za-z]:\\' -and (Test-Path -LiteralPath $values.directory -PathType Container)) {
    Assert-LocalPath $values.directory
    $sameDirectory = [TowavueUpdatePaths]::Expand($values.directory) -eq $InstallDirectory
}
if ($values.id -cnotmatch '^towavue-local-[0-9a-f]{64}$' -or -not $sameDirectory) { throw 'Installation marker belongs to another payload or directory.' }
$oldFiles = Read-Inventory $InstallDirectory $values.id
$newFiles = Read-Inventory $IncomingPayloadDirectory $IncomingOwnershipId
# Incoming staging is owned build output: unlike the installed folder, extra files
# are not user data and would make the proposed replacement incomplete/ambiguous.
$pending = [Collections.Generic.Queue[string]]::new()
$pending.Enqueue($IncomingPayloadDirectory)
$seen = @{}
while ($pending.Count) {
    foreach ($item in Get-ChildItem -LiteralPath $pending.Dequeue() -Force) {
        if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Incoming payload contains a reparse point.' }
        if ($item.PSIsContainer) { $pending.Enqueue($item.FullName); continue }
        $name = $item.FullName.Substring($IncomingPayloadDirectory.Length + 1).Replace('\','/')
        if (-not $newFiles.ContainsKey($name)) { throw "Incoming payload contains an unlisted file: $name" }
        $seen.Add($name,$true)
    }
}
if ($seen.Count -ne $newFiles.Count) { throw 'Incoming payload coverage mismatch.' }
$oldFiles.Add($marker.name,$marker)
$oldFiles.Add('Uninstall.exe',(Get-Record $InstallDirectory 'Uninstall.exe'))
$actions = [Collections.Generic.List[object]]::new()
$names = [string[]]@($newFiles.Keys + $oldFiles.Keys | Sort-Object -Unique)
[Array]::Sort($names,[StringComparer]::Ordinal)
foreach ($name in $names) {
    if ($name -in @('Uninstall.exe','towavue-install.ini')) { continue }
    $old = $oldFiles[$name]
    $next = $newFiles[$name]
    if (-not $old) {
        $destination = Join-Path $InstallDirectory $name
        Assert-LocalPath $destination
        if (Test-Path -LiteralPath $destination) { throw "New payload would overwrite an unowned path: $name" }
    }
    $action = if (-not $old) { 'add' } elseif (-not $next) { 'remove' } elseif ($old.sha256 -ceq $next.sha256 -and $old.bytes -eq $next.bytes) { 'keep' } else { 'replace' }
    $actions.Add([pscustomobject]@{name=$name;action=$action;before=$old;after=$next})
}
# A read/write exclusive open performs no writes, but detects access and sharing
# failures (including a running executable). This snapshot is not a retained lock;
# an eventual updater must repeat validation immediately before replacement.
foreach ($name in $oldFiles.Keys) {
    $stream = [IO.File]::Open((Join-Path $InstallDirectory $name),[IO.FileMode]::Open,[IO.FileAccess]::ReadWrite,[IO.FileShare]::None)
    $stream.Dispose()
}
[pscustomobject]@{
    schema_version=1
    scope='Read-only update snapshot, not authorization or an executed update. Revalidate before a recoverable transaction; no old uninstaller was executed.'
    install_directory=$InstallDirectory
    incoming_directory=$IncomingPayloadDirectory
    installed_ownership_id=$values.id
    incoming_ownership_id=$IncomingOwnershipId
    identical_payload=($values.id -ceq $IncomingOwnershipId)
    actions=@($actions)
    old_metadata=@($oldFiles['towavue-install.ini'],$oldFiles['Uninstall.exe'])
    old_uninstaller_scope='Current bytes captured for backup only; original uninstaller identity was not recorded by this marker format.'
    backup_bytes=($oldFiles.Values | Measure-Object bytes -Sum).Sum
    incoming_bytes=($newFiles.Values | Measure-Object bytes -Sum).Sum
} | ConvertTo-Json -Depth 8

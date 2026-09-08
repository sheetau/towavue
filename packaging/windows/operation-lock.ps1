[CmdletBinding()]
param(
    [switch]$EmitName,
    [string]$RegistrySubKey='Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation'
)

function Get-TowavueOperationName([string]$RegistrySubKey) {
    if ($RegistrySubKey -ne 'Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue-evaluation' -and
        $RegistrySubKey -ne 'towavue-setup-fixture-v1' -and
        $RegistrySubKey -notmatch '^Software\\towavue\\InstallerTests\\[0-9a-f]{32}$') { throw 'Operation identity is outside the owned namespace.' }
    $user = [Security.Principal.WindowsIdentity]::GetCurrent()
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $identity = $user.User.Value + '|' + $RegistrySubKey.ToUpperInvariant()
        return 'Global\towavue-operation-' + [BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($identity))).Replace('-','')
    } finally { $sha.Dispose(); $user.Dispose() }
}

function New-TowavueOperationLease([string]$RegistrySubKey) {
    $created = $false
    # Existence is the lease, not thread ownership: NSIS's worker and UI cleanup
    # may differ. Every participant closes an existing object and refuses work;
    # only its creator retains a handle. Process exit also releases that handle.
    $lease = [Threading.Mutex]::new($false,(Get-TowavueOperationName $RegistrySubKey),[ref]$created)
    if (-not $created) { $lease.Dispose(); throw 'Another installation operation is active.' }
    return $lease
}

if ($EmitName) {
    try { [Console]::Write((Get-TowavueOperationName $RegistrySubKey)) }
    catch { [Console]::Error.WriteLine('Operation identity could not be resolved.'); exit 20 }
}

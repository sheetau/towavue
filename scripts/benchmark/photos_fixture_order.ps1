param(
    [Parameter(Mandatory=$true)][string]$FixtureRoot,
    [Parameter(Mandatory=$true)][string]$Folder
)
$ErrorActionPreference='Stop'
$resolved=(Resolve-Path -LiteralPath $Folder).Path
$allowed=(Resolve-Path -LiteralPath $FixtureRoot).Path.TrimEnd('\')+'\'
if(-not $resolved.StartsWith($allowed,[StringComparison]::OrdinalIgnoreCase)) {
    throw 'Only a child of the new fixture root is allowed'
}
$shell=New-Object -ComObject Shell.Application
$before=@($shell.Windows() | ForEach-Object {$_.HWND})
$shell.Explore($resolved)
$owned=$null
$deadline=[DateTime]::UtcNow.AddSeconds(12)
try {
    while([DateTime]::UtcNow -lt $deadline) {
        $matches=@($shell.Windows() | Where-Object {
            $_.HWND -notin $before -and $_.Document.Folder.Self.Path -eq $resolved
        })
        if($matches.Count -gt 1) {throw 'Ambiguous new fixture Explorer window'}
        if($matches.Count -eq 1) {$owned=$matches[0];break}
        Start-Sleep -Milliseconds 100
    }
    if($null -eq $owned) {throw 'No uniquely owned fixture Explorer window'}
    $owned.Document.SortColumns='prop:System.ItemNameDisplay;'
    Start-Sleep -Milliseconds 300
    if($owned.Document.SortColumns -ne 'prop:System.ItemNameDisplay;') {
        throw 'Fixture sort was not applied'
    }
} finally {
    if($null -ne $owned -and $owned.HWND -notin $before -and $owned.Document.Folder.Self.Path -eq $resolved) {
        $owned.Quit()
    }
}

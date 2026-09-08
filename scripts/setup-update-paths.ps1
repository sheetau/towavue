if (-not ('TowavueUpdatePaths' -as [type])) {
    # Expand existing DOS aliases before comparing roots or inventory names. The
    # synchronous read-only API retains no handle or pointer after this call.
    Add-Type @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
using System.Text;
public static class TowavueUpdatePaths {
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    private static extern uint GetLongPathNameW(string path, StringBuilder output, uint size);
    public static string Expand(string path) {
        var output = new StringBuilder(32768);
        uint length = GetLongPathNameW(path, output, (uint)output.Capacity);
        if (length == 0) throw new Win32Exception(Marshal.GetLastWin32Error());
        if (length >= output.Capacity) throw new ArgumentException("Update path is too long.");
        return output.ToString();
    }
}
'@
}
function Assert-LocalPath([string]$Path) {
    if ($Path -notmatch '^[A-Za-z]:[\\/]' -or $Path.TrimEnd('\','/').Length -le 2) { throw 'Use an absolute dedicated local directory.' }
    $current = $Path
    while ($current) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Update paths must not traverse reparse points.' }
            if ($current -ne $Path -and -not $item.PSIsContainer) { throw 'Update path ancestor is not a directory.' }
        }
        $parent = Split-Path -Parent $current
        if ($parent -eq $current) { break }
        $current = $parent
    }
}
function Assert-Name([string]$Name) {
    if ($Name -notmatch '^[A-Za-z0-9_.-][A-Za-z0-9_./+~-]*$') { throw 'Unsafe update inventory name.' }
    foreach ($part in $Name.Split('/')) {
        if (-not $part -or $part.EndsWith('.') -or $part -in @('.','..') -or
            $part -match '^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(\.|$)') { throw 'Unsafe update inventory name.' }
    }
    if ($Name -in @('Uninstall.exe','towavue-install.ini','licenses/INSTALLED-FILES.json')) { throw 'Inventory claims reserved installer metadata.' }
}
function Get-Record([string]$Root,[string]$Name) {
    $path = Join-Path $Root $Name
    Assert-LocalPath $path
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing update file: $Name" }
    if ([TowavueUpdatePaths]::Expand($path) -ne $path) { throw 'Update inventory uses a short-name alias.' }
    $item = Get-Item -LiteralPath $path
    return [pscustomobject]@{name=$Name;bytes=$item.Length;sha256=(Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant()}
}

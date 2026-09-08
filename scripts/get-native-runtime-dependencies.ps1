[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string[]]$EntryPoints,
    [Parameter(Mandatory = $true)][string[]]$SearchDirectories,
    [Parameter(Mandatory = $true)][string]$ObjdumpExecutable
)

$ErrorActionPreference = 'Stop'
$ObjdumpExecutable = (Resolve-Path -LiteralPath $ObjdumpExecutable).Path
$directories = @($SearchDirectories | ForEach-Object { (Resolve-Path -LiteralPath $_).Path } | Select-Object -Unique)
$systemDirectory = Join-Path $env:SystemRoot 'System32'
$knownDlls = @((Get-ItemProperty 'HKLM:/SYSTEM/CurrentControlSet/Control/Session Manager/KnownDLLs').PSObject.Properties |
    Where-Object { $_.Value -is [string] -and $_.Value -like '*.dll' } | ForEach-Object { $_.Value })
$queue = [Collections.Generic.Queue[string]]::new()
foreach ($entry in $EntryPoints) { $queue.Enqueue((Resolve-Path -LiteralPath $entry).Path) }
$visited = @{}
while ($queue.Count -gt 0) {
    $path = $queue.Dequeue()
    $name = [IO.Path]::GetFileName($path)
    if ($visited.ContainsKey($name)) {
        if ($visited[$name] -ne $path) { throw "Different runtime files share a basename: $name" }
        continue
    }
    $visited[$name] = $path
    $headers = @(& $ObjdumpExecutable -p $path)
    if ($LASTEXITCODE -ne 0 -or ($headers -join "`n") -notmatch 'file format pei-x86-64') {
        throw "Cannot inspect an x86-64 PE runtime file: $path"
    }
    $imports = @(foreach ($line in $headers) {
        if ($line -match '^\s*DLL Name:\s*(\S+)\s*$') { $Matches[1] }
    })
    $imports = @($imports | Sort-Object -Unique)
    $resolved = @(foreach ($import in $imports) {
        if ($import -notmatch '^[A-Za-z0-9_.+-]+\.dll$') { throw "Unexpected imported DLL name: $import" }
        $candidates = @($directories | ForEach-Object {
            $candidate = Join-Path $_ $import
            if (Test-Path -LiteralPath $candidate -PathType Leaf) { (Resolve-Path -LiteralPath $candidate).Path }
        } | Select-Object -Unique)
        if ($import -match '^(api-ms-win-|ext-ms-win-)') {
            if ($candidates.Count) { throw "Local file shadows a Windows API set: $import" }
            [pscustomobject]@{ name = $import; kind = 'windows-api-set'; path = $null }
        }
        elseif ($import -in $knownDlls) {
            if ($candidates.Count) { throw "Local file conflicts with a system DLL: $import" }
            [pscustomobject]@{ name = $import; kind = 'windows-known-dll'; path = (Join-Path $systemDirectory $import) }
        }
        elseif ($candidates.Count -eq 0 -and (Test-Path -LiteralPath (Join-Path $systemDirectory $import) -PathType Leaf)) {
            [pscustomobject]@{ name = $import; kind = 'host-system'; path = (Join-Path $systemDirectory $import) }
        }
        else {
            if ($candidates.Count -ne 1) { throw "Expected exactly one allowed location for ${import}; found $($candidates.Count)." }
            $queue.Enqueue($candidates[0])
            [pscustomobject]@{ name = $import; kind = 'runtime'; path = $candidates[0] }
        }
    })
    [pscustomobject]@{
        name = $name
        path = $path
        bytes = (Get-Item -LiteralPath $path).Length
        sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        imports = $resolved
    }
}

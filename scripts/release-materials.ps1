# Shared offline release input checks. No discovery, download or installation.
$ErrorActionPreference = 'Stop'

function Assert-ReleaseVersion([string]$Version) {
    if ($Version -cnotmatch '^(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})\.(0|[1-9][0-9]{0,4})\z' -or
        @($Version.Split('.') | Where-Object { [int]$_ -gt 65535 }).Count) { throw 'Release version must be canonical x.y.z with each component at most 65535.' }
}

function Resolve-ReleasePath([string]$Path) {
    $resolved = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
    if ($resolved.StartsWith('\\')) { throw 'Release files require local drive paths.' }
    $current = $resolved
    while ($current) {
        if ((Test-Path -LiteralPath $current) -and ((Get-Item -LiteralPath $current -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw 'Release files must not traverse reparse points.' }
        $parent = Split-Path -Parent $current
        if ($parent -eq $current) { break }
        $current = $parent
    }
    return $resolved
}

function Assert-ReleaseName([string]$Name) {
    if ($Name -cnotmatch '^[A-Za-z0-9_.-][A-Za-z0-9_./+~-]*\z' -or $Name -match '(^|/)\.{1,2}(/|$)' -or $Name.EndsWith('/')) { throw 'Unsafe release material path.' }
}

function Assert-ReleaseOutput([string]$Output, [string[]]$Inputs) {
    if (Test-Path -LiteralPath $Output) { throw 'Use a fresh release output directory.' }
    if (-not (Test-Path -LiteralPath (Split-Path -Parent $Output) -PathType Container)) { throw 'Release output parent must exist.' }
    foreach ($path in $Inputs) {
        if ($path -eq $Output -or $path.StartsWith($Output.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase) -or
            $Output.StartsWith($path.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Release output overlaps an input.' }
    }
}

function Get-ReleaseRecord([string]$Path, [string]$Name) {
    Assert-ReleaseName $Name
    $resolved = Resolve-ReleasePath $Path
    $item = Get-Item -LiteralPath $resolved -Force
    if ($item.PSIsContainer) { throw 'Expected a regular release file.' }
    return [pscustomobject][ordered]@{name=$Name;bytes=$item.Length;sha256=(Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash.ToLowerInvariant()}
}

function Assert-ReleaseFile([string]$Path, $Record) {
    $actual = Get-ReleaseRecord $Path $Record.name
    if ($actual.bytes -ne $Record.bytes -or $actual.sha256 -cne $Record.sha256) { throw "Release material identity differs: $($Record.name)" }
}

function Write-ReleaseJson([string]$Path, $Value) {
    [IO.File]::WriteAllText($Path, (ConvertTo-Json -InputObject $Value -Depth 40) + "`n", [Text.UTF8Encoding]::new($false))
}

function Get-ReleaseTree([string]$Root) {
    $Root = Resolve-ReleasePath $Root
    $pending = [Collections.Generic.Queue[string]]::new()
    $pending.Enqueue($Root)
    $records = @{}
    while ($pending.Count) {
        $directory = $pending.Dequeue()
        foreach ($item in Get-ChildItem -LiteralPath $directory -Force) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Release material links are not allowed.' }
            if ($item.PSIsContainer) { $pending.Enqueue($item.FullName); continue }
            $name = $item.FullName.Substring($Root.Length + 1).Replace('\','/')
            $records.Add($name, (Get-ReleaseRecord $item.FullName $name))
        }
    }
    $names = [string[]]@($records.Keys)
    [Array]::Sort($names, [StringComparer]::Ordinal)
    $lines = @($names | ForEach-Object { "$_ $($records[$_].bytes) $($records[$_].sha256)" })
    $hash = [Security.Cryptography.SHA256]::Create()
    try { $digest = [BitConverter]::ToString($hash.ComputeHash([Text.Encoding]::UTF8.GetBytes(($lines -join "`n") + "`n"))).Replace('-','').ToLowerInvariant() }
    finally { $hash.Dispose() }
    return [pscustomobject]@{files=@($names | ForEach-Object { $records[$_] });bytes=($records.Values | Measure-Object bytes -Sum).Sum;tree_sha256=$digest}
}

function Get-ReleaseSourceIdentity([string]$Repository) {
    $status = @(& git -C $Repository status --porcelain=v1 --untracked-files=normal)
    if ($LASTEXITCODE -ne 0 -or $status.Count) { throw 'Release assembly requires a clean committed worktree.' }
    $commit = & git -C $Repository rev-parse --verify HEAD
    if ($LASTEXITCODE -ne 0 -or $commit -cnotmatch '^[0-9a-f]{40}$') { throw 'Cannot identify release source commit.' }
    $manifest = Get-Content -LiteralPath (Join-Path $Repository 'Cargo.toml') -Raw -Encoding UTF8
    if ($manifest -notmatch '(?ms)^\[workspace\.package\]\s*\r?\nversion = "([^"]+)"') { throw 'Cannot identify workspace version.' }
    $version = $Matches[1]
    Assert-ReleaseVersion $version
    return [pscustomobject]@{commit=$commit;version=$version}
}

function Assert-ReleaseSourceArchive([string]$Archive, [string]$Repository, [string]$Commit) {
    $tree = @(& git -C $Repository -c core.quotepath=false ls-tree -r --full-tree $Commit)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot inspect committed release files.' }
    $blobs = @{}
    foreach ($line in $tree) {
        if ($line -cnotmatch '^100(644|755) blob ([0-9a-f]{40})\t(.+)$') { throw 'Source archive requires regular committed files.' }
        $name = $Matches[3]; $hash = $Matches[2]
        Assert-ReleaseName $name
        $blobs.Add(('towavue/' + $name), $hash)
    }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = [IO.Compression.ZipFile]::OpenRead($Archive)
    $sha = [Security.Cryptography.SHA1]::Create()
    try {
        $seen = @{}
        foreach ($entry in $zip.Entries) {
            if ($entry.FullName.EndsWith('/')) { continue }
            if (-not $blobs.ContainsKey($entry.FullName) -or $seen.ContainsKey($entry.FullName)) { throw 'Source archive coverage differs from Git.' }
            $stream = $entry.Open(); $memory = [IO.MemoryStream]::new()
            try { $stream.CopyTo($memory); $bytes = $memory.ToArray() }
            finally { $memory.Dispose(); $stream.Dispose() }
            $header = [Text.Encoding]::ASCII.GetBytes('blob ' + $bytes.Length + [char]0)
            $sha.Initialize()
            [void]$sha.TransformBlock($header,0,$header.Length,$header,0)
            [void]$sha.TransformFinalBlock($bytes,0,$bytes.Length)
            $hash = [BitConverter]::ToString($sha.Hash).Replace('-','').ToLowerInvariant()
            if ($hash -cne $blobs[$entry.FullName]) { throw 'Source archive differs from committed Git blob.' }
            $seen.Add($entry.FullName,$true)
        }
        if ($seen.Count -ne $blobs.Count) { throw 'Source archive omits committed files.' }
    }
    finally { $sha.Dispose(); $zip.Dispose() }
    return $blobs.Count
}

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ConfigPath,
    [Parameter(Mandatory = $true)][string]$MsysRoot,
    [Parameter(Mandatory = $true)][string]$EvidencePath
)

$ErrorActionPreference = 'Stop'
$ConfigPath = (Resolve-Path -LiteralPath $ConfigPath).Path
$MsysRoot = (Resolve-Path -LiteralPath $MsysRoot).Path
$EvidencePath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($EvidencePath)
if (Test-Path -LiteralPath $EvidencePath) { throw 'Preserve existing static-link evidence; use a fresh configured build.' }
$utf8 = [Text.UTF8Encoding]::new($false, $true)
$before = [IO.File]::ReadAllBytes($ConfigPath)
$text = $utf8.GetString($before)
if ($text.Contains('-Wl,--start-group') -or $text.Contains('-DKVZ_STATIC_LIB')) {
    throw 'Static link policy requires a fresh, unmodified configure result.'
}
$lines = [regex]::Matches($text, '[^\r\n]*(?:\r\n|\n|$)') | Where-Object { $_.Length -gt 0 }
$globalFlags = @($lines | Where-Object { $_.Value.StartsWith('LDFLAGS=') })
if ($globalFlags.Count -ne 1 -or @($lines | Where-Object { $_.Value.StartsWith('CFLAGS=') }).Count -ne 1 -or
    @($lines | Where-Object { $_.Value -match '^EXTRALIBS=\r?\n?$' }).Count -ne 1) {
    throw 'Unexpected pinned FFmpeg make configuration.'
}
$globalDirectories = @([regex]::Matches($globalFlags[0].Value, '(?:^| )-L([^ \r\n]+)') | ForEach-Object { $_.Groups[1].Value })
$gcc = Join-Path $MsysRoot 'mingw64/bin/gcc.exe'
$choices = [Collections.Generic.List[object]]::new()
$resolved = @{}
$shared = @('aribcaption', 'hwy', 'zmq', 'placebo')

function Find-Archive([string]$Name, [string[]]$Directories) {
    $filename = 'lib' + $Name + '.a'
    foreach ($directory in $Directories) {
        $candidate = Join-Path $directory $filename
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return (Resolve-Path -LiteralPath $candidate).Path }
    }
    if (-not $resolved.ContainsKey($filename)) {
        $result = @(& $gcc "-print-file-name=$filename")
        if ($LASTEXITCODE -ne 0 -or $result.Count -ne 1) { throw "Cannot resolve compiler archive: $filename" }
        $resolved[$filename] = if ($result[0] -ne $filename -and (Test-Path -LiteralPath $result[0] -PathType Leaf)) {
            (Resolve-Path -LiteralPath $result[0]).Path
        } else { $null }
    }
    return $resolved[$filename]
}

$after = [Text.StringBuilder]::new()
foreach ($match in $lines) {
    $line = $match.Value
    if ($line.StartsWith('CFLAGS=')) {
        # Installed static consumers need these declarations. Do not modify
        # configure feature macros or the libraries' installed headers.
        $line = 'CFLAGS= -DCHROMAPRINT_NODLL  -DKVZ_STATIC_LIB -DLIBTWOLAME_STATIC ' + $line.Substring(7)
    } elseif ($line -match '^(EXTRALIBS(?:-[\w-]+)?=)([^\r\n]*)(\r?\n)?$') {
        $key = $Matches[1]
        $body = $Matches[2]
        $ending = $Matches[3]
        $directories = @($globalDirectories) + @([regex]::Matches($body, '(?:^| )-L([^ \r\n]+)') |
            ForEach-Object { $_.Groups[1].Value }) + @(Join-Path $MsysRoot 'mingw64/lib')
        # The qualified profile retains a leading -l token immediately after
        # '='; only whitespace-delimited library tokens select explicit archives.
        $body = [regex]::Replace('=' + $body, '(?<!\S)-l([A-Za-z0-9_+.\-]+)(?=\s|$)', {
            param($libraryMatch)
            $name = $libraryMatch.Groups[1].Value
            $archive = Find-Archive $name $directories
            $replacement = if ($archive -and $name -notin $shared) { '-l:lib' + $name + '.a' } else { $libraryMatch.Value }
            $choices.Add([pscustomobject]@{
                variable = $key.TrimEnd('='); library = $name; replacement = $replacement
                archive = $archive
                sha256 = if ($archive) { (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() } else { $null }
            })
            return $replacement
        }).Substring(1)
        if ($key -eq 'EXTRALIBS=') {
            foreach ($name in @('avrt', 'stdc++', 'gomp')) {
                $archive = Find-Archive $name $directories
                if (-not $archive) { throw "Missing required static/import archive: $name" }
                $choices.Add([pscustomobject]@{
                    variable = 'EXTRALIBS'; library = $name; replacement = '-l:lib' + $name + '.a'
                    archive = $archive; sha256 = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
                })
            }
            $body = '-l:libavrt.a -l:libstdc++.a -l:libgomp.a'
        }
        if ($body.Length) { $body = '-Wl,--start-group ' + $body + ' -Wl,--end-group' }
        $line = $key + $body + $ending
    }
    [void]$after.Append($line)
}
# Inter-FFmpeg LDLIBS and every other configure variable are untouched.
$output = $utf8.GetBytes($after.ToString())
if ([Convert]::ToBase64String($output) -eq [Convert]::ToBase64String($before)) { throw 'No static link policy was applied.' }
[IO.File]::WriteAllBytes($ConfigPath, $output)
$evidence = [ordered]@{
    profile = 'seek-optimized-v1'
    before_sha256 = [BitConverter]::ToString([Security.Cryptography.SHA256]::Create().ComputeHash($before)).Replace('-', '').ToLowerInvariant()
    after_sha256 = (Get-FileHash -LiteralPath $ConfigPath -Algorithm SHA256).Hash.ToLowerInvariant()
    shared_exceptions = $shared
    choices = @($choices)
}
[IO.File]::WriteAllText($EvidencePath, ($evidence | ConvertTo-Json -Depth 6) + "`n", $utf8)
Write-Output "Applied pinned static-preferred external linking: $ConfigPath"

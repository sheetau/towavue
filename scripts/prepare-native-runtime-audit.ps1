[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
    [Parameter(Mandatory = $true)][string]$MsysRoot,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$CacheDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$RuntimeDirectory = (Resolve-Path -LiteralPath $RuntimeDirectory).Path
$MsysRoot = (Resolve-Path -LiteralPath $MsysRoot).Path
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh runtime audit directory; existing output is preserved.' }
if (-not $CacheDirectory) { $CacheDirectory = Join-Path $repositoryRoot 'vendor/msys2/packages-20260908' }
$CacheDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($CacheDirectory)
$base = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-toolchain-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$media = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/msys2-media-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$records = @{}
foreach ($record in @($base.packages) + @($media.packages)) { $records.Add($record.name, $record) }
$tar = Join-Path $env:SystemRoot 'System32/tar.exe'
$savedPath = $env:PATH
try {
    $env:PATH = "$MsysRoot/mingw64/bin;$MsysRoot/usr/bin;" + (Join-Path $env:SystemRoot 'System32')
    $graph = @(& (Join-Path $PSScriptRoot 'get-native-runtime-dependencies.ps1') `
        -EntryPoints "$RuntimeDirectory/ffmpeg.exe","$RuntimeDirectory/ffprobe.exe" `
        -SearchDirectories $RuntimeDirectory -ObjdumpExecutable "$MsysRoot/mingw64/bin/objdump.exe")
    $ffmpegNames = @('ffmpeg.exe', 'ffprobe.exe', 'avcodec-63.dll', 'avdevice-63.dll', 'avfilter-12.dll',
        'avformat-63.dll', 'avutil-61.dll', 'swresample-7.dll', 'swscale-10.dll')
    foreach ($name in $ffmpegNames) {
        if ($name -notin $graph.name) { throw "Missing expected native FFmpeg entry: $name" }
    }
    $packageFiles = @($graph | Where-Object { $_.name -notin $ffmpegNames } | Sort-Object name)
    $owners = @(& "$MsysRoot/usr/bin/pacman.exe" -Qqo @($packageFiles | ForEach-Object { '/mingw64/bin/' + $_.name }))
    if ($LASTEXITCODE -ne 0 -or $owners.Count -ne $packageFiles.Count) { throw 'Runtime ownership lookup failed.' }
    $byPackage = @{}
    for ($index = 0; $index -lt $packageFiles.Count; $index++) {
        $owner = $owners[$index]
        if (-not $records.ContainsKey($owner)) { throw "Unpinned runtime package: $owner" }
        if (-not $byPackage.ContainsKey($owner)) { $byPackage[$owner] = @() }
        $byPackage[$owner] += $packageFiles[$index]
    }
    # Verify every archive before creating audit output or extracting selected members.
    foreach ($owner in $byPackage.Keys) {
        $record = $records[$owner]
        $archive = Join-Path $CacheDirectory ([uri]$record.url).Segments[-1]
        if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) { throw "Missing runtime package archive: $archive" }
        if ((Get-Item -LiteralPath $archive).Length -ne $record.bytes -or
            (Get-FileHash -LiteralPath $archive).Hash -ne $record.sha256) { throw "Runtime package archive checksum mismatch: $archive" }
    }
    New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
    $packages = @(foreach ($owner in ($byPackage.Keys | Sort-Object)) {
        $record = $records[$owner]
        $archive = Join-Path $CacheDirectory ([uri]$record.url).Segments[-1]
        $entries = @(& $tar -tf $archive)
        if ($LASTEXITCODE -ne 0) { throw "Package listing failed: $owner" }
        $notices = @($entries | Where-Object { $_ -match '^mingw64/share/licenses/.+[^/]$' } | Sort-Object)
        $selected = @('.BUILDINFO', '.PKGINFO') + @($byPackage[$owner] | ForEach-Object { 'mingw64/bin/' + $_.name }) + $notices
        foreach ($entry in $selected) {
            if ($entry -notin $entries -or $entry -match '(^|/)\.\.(/|$)|[:\\*?\[\]]') { throw "Unsafe or missing package member: $entry" }
        }
        $types = @(& $tar -tvf $archive @selected)
        if ($LASTEXITCODE -ne 0 -or $types.Count -ne $selected.Count -or @($types | Where-Object { -not $_.StartsWith('-') }).Count) {
            throw "Expected only regular selected package members: $owner"
        }
        $directory = Join-Path $OutputDirectory $owner
        New-Item -ItemType Directory -Path $directory | Out-Null
        & $tar -xf $archive -C $directory @selected
        if ($LASTEXITCODE -ne 0) { throw "Selected package extraction failed: $owner" }
        foreach ($file in $byPackage[$owner]) {
            $path = Join-Path $directory ('mingw64/bin/' + $file.name)
            if ((Get-Item -LiteralPath $path).Length -ne $file.bytes -or (Get-FileHash -LiteralPath $path).Hash -ne $file.sha256) {
                throw "Runtime DLL does not match its pinned package: $($file.name)"
            }
        }
        $metadata = @(Get-Content -LiteralPath "$directory/.PKGINFO" -Encoding UTF8)
        $build = @(Get-Content -LiteralPath "$directory/.BUILDINFO" -Encoding UTF8)
        if ("pkgname = $owner" -notin $metadata -or "pkgver = $($record.version)" -notin $metadata -or
            "pkgname = $owner" -notin $build -or "pkgver = $($record.version)" -notin $build) { throw "Package identity mismatch: $owner" }
        $recipe = @($build | Where-Object { $_ -match '^pkgbuild_sha256sum = [a-f0-9]{64}$' })
        $pkgbase = @($metadata | Where-Object { $_ -match '^pkgbase = [a-z0-9+_.-]+$' })
        if ($recipe.Count -ne 1 -or $pkgbase.Count -ne 1) { throw "Missing unique package build identity: $owner" }
        [pscustomobject]@{
            name = $owner; version = $record.version; pkgbase = $pkgbase[0].Substring(10)
            archive_url = $record.url; archive_bytes = $record.bytes; archive_sha256 = $record.sha256
            recipe_sha256 = $recipe[0].Substring(21)
            pkginfo_sha256 = (Get-FileHash -LiteralPath "$directory/.PKGINFO").Hash.ToLowerInvariant()
            buildinfo_sha256 = (Get-FileHash -LiteralPath "$directory/.BUILDINFO").Hash.ToLowerInvariant()
            license_labels = @($metadata | Where-Object { $_ -like 'license = *' } | ForEach-Object { $_.Substring(10) })
            runtime_dlls = @($byPackage[$owner].name | Sort-Object)
            package_notices = @(foreach ($name in $notices) {
                $path = Join-Path $directory $name
                [pscustomobject]@{ name = $name; bytes = (Get-Item -LiteralPath $path).Length; sha256 = (Get-FileHash -LiteralPath $path).Hash.ToLowerInvariant() }
            })
        }
    })
    $result = [pscustomobject]@{
        schema_version = 1
        scope = 'Observed helper PE closure and hash-matched package files. Not a complete static/embedded dependency graph, corresponding-source bundle, signature verification or distribution approval.'
        runtime = @($graph | Sort-Object name | ForEach-Object {
            [pscustomobject]@{ name = $_.name; bytes = $_.bytes; sha256 = $_.sha256; imports = @($_.imports | Select-Object name,kind) }
        })
        packages = $packages
    }
    [IO.File]::WriteAllText((Join-Path $OutputDirectory 'INDEX.json'), ($result | ConvertTo-Json -Depth 8) + "`n", [Text.UTF8Encoding]::new($false))
    Write-Output "Runtime audit: $OutputDirectory"
    Write-Output "$($graph.Count) runtime files; $($packageFiles.Count) package DLLs verified against $($packages.Count) pinned archives."
    Write-Output 'Audit contains DLL inspection copies: do not ship the audit directory as an installer payload.'
}
finally { $env:PATH = $savedPath }

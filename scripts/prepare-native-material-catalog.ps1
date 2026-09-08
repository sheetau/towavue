[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$MaterialsDirectory,
    [Parameter(Mandatory = $true)][string]$OutputDirectory,
    [string]$PackageDirectory,
    [string]$RecipeDirectory
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$inventoryPath = Join-Path $repositoryRoot 'docs/native-material-catalog.json'
$inventory = Get-Content -LiteralPath $inventoryPath -Raw -Encoding UTF8 | ConvertFrom-Json
$auditPath = Join-Path $repositoryRoot 'docs/native-runtime-package-audit.json'
$audit = Get-Content -LiteralPath $auditPath -Raw -Encoding UTF8 | ConvertFrom-Json
$recipesPath = Join-Path $repositoryRoot 'docs/native-runtime-recipes.json'
$recipes = Get-Content -LiteralPath $recipesPath -Raw -Encoding UTF8 | ConvertFrom-Json
if ($inventory.schema_version -ne 1 -or $inventory.kits.Count -ne 13 -or
    $inventory.excluded_package -ne 'mingw-w64-x86_64-zvbi') { throw 'Incomplete native material catalog.' }
if (-not $PackageDirectory) { $PackageDirectory = Join-Path $repositoryRoot 'vendor/msys2/packages-20260908' }
if (-not $RecipeDirectory) { $RecipeDirectory = Join-Path $repositoryRoot 'vendor/msys2/runtime-recipes-20260908' }
$MaterialsDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($MaterialsDirectory)
$PackageDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($PackageDirectory)
$RecipeDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($RecipeDirectory)
$OutputDirectory = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Use a fresh native catalog output directory.' }
foreach ($inputRoot in @($MaterialsDirectory,$PackageDirectory,$RecipeDirectory)) {
    if ($inputRoot -eq $OutputDirectory -or
        $inputRoot.StartsWith($OutputDirectory.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase) -or
        $OutputDirectory.StartsWith($inputRoot.TrimEnd('\','/') + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) { throw 'Native catalog output overlaps an input.' }
}
$utf8 = [Text.UTF8Encoding]::new($false)
function Assert-Path([string]$Name) {
    if (-not $Name -or $Name -match '(^/|[\\:"]|(^|/)\.\.(/|$))') { throw 'Invalid native catalog path.' }
}
function Assert-File([string]$Path, [string]$Hash) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) { throw "Missing native catalog input: $Path" }
    if ((Get-Item -LiteralPath $Path).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Native catalog links are not allowed.' }
    if ((Get-FileHash -LiteralPath $Path).Hash -ne $Hash) { throw "Native catalog checksum mismatch: $Path" }
}
function Get-Tree([string]$Root) {
    if (-not (Test-Path -LiteralPath $Root -PathType Container)) { throw "Missing native catalog kit: $Root" }
    $pending = [Collections.Generic.Queue[string]]::new()
    $pending.Enqueue($Root)
    $files = @{}
    while ($pending.Count) {
        $directory = $pending.Dequeue()
        if ((Get-Item -LiteralPath $directory).Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Native catalog links are not allowed.' }
        foreach ($item in Get-ChildItem -LiteralPath $directory -Force) {
            if ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Native catalog links are not allowed.' }
            if ($item.PSIsContainer) { $pending.Enqueue($item.FullName); continue }
            $name = $item.FullName.Substring($Root.Length + 1).Replace('\','/')
            Assert-Path $name
            $files[$name] = [pscustomobject]@{name=$name;bytes=$item.Length;sha256=(Get-FileHash -LiteralPath $item.FullName).Hash.ToLowerInvariant()}
        }
    }
    $names = [string[]]@($files.Keys)
    [Array]::Sort($names,[StringComparer]::Ordinal)
    $lines = @($names | ForEach-Object { "$($_) $($files[$_].bytes) $($files[$_].sha256)" })
    $sha = [Security.Cryptography.SHA256]::Create()
    try { $hash = [BitConverter]::ToString($sha.ComputeHash($utf8.GetBytes(($lines -join "`n") + "`n"))).Replace('-','').ToLowerInvariant() }
    finally { $sha.Dispose() }
    return [pscustomobject]@{files=@($names | ForEach-Object { $files[$_] });tree_sha256=$hash;bytes=($files.Values | Measure-Object bytes -Sum).Sum}
}
function Assert-Kit($Tree, $Kit) {
    if ($Tree.files.Count -ne $Kit.files -or $Tree.bytes -ne $Kit.bytes -or $Tree.tree_sha256 -ne $Kit.tree_sha256) { throw "Native catalog kit mismatch: $($Kit.name)" }
}
function Get-Link([string]$Path) { return (($Path.Split('/') | ForEach-Object { [uri]::EscapeDataString($_) }) -join '/') }

$packages = @($audit.packages | Where-Object name -ne $inventory.excluded_package)
if ($packages.Count -ne $inventory.package_count -or @($packages.package_notices).Count -ne $inventory.package_notice_count) { throw 'Native catalog package coverage mismatch.' }
$packageInputs = @{}
foreach ($package in $packages) {
    Assert-Path $package.name
    $recipe = @($recipes.recipes | Where-Object package -eq $package.name)
    if ($recipe.Count -ne 1 -or $recipe[0].sha256 -ne $package.recipe_sha256) { throw 'Stale native catalog recipe mapping.' }
    Assert-Path $recipe[0].name
    $archiveName = [uri]::UnescapeDataString(([uri]$package.archive_url).Segments[-1])
    Assert-Path $archiveName
    $archivePath = Join-Path $PackageDirectory $archiveName
    Assert-File $archivePath $package.archive_sha256
    Assert-File (Join-Path $RecipeDirectory $recipe[0].name) $recipe[0].sha256
    foreach ($notice in $package.package_notices) { Assert-Path $notice.name }
    if ($packageInputs.ContainsKey($package.name)) { throw 'Duplicate native catalog package.' }
    $packageInputs[$package.name] = @{archive=$archivePath;recipe=$recipe[0]}
}
$trees = @{}
foreach ($kit in $inventory.kits) {
    Assert-Path $kit.name
    if ($trees.ContainsKey($kit.name)) { throw 'Duplicate native catalog kit.' }
    $tree = Get-Tree (Join-Path $MaterialsDirectory $kit.name)
    Assert-Kit $tree $kit
    $trees[$kit.name] = $tree
}

New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
foreach ($kit in $inventory.kits) {
    $destination = Join-Path $OutputDirectory ('materials/' + $kit.name)
    foreach ($file in $trees[$kit.name].files) {
        $path = Join-Path $destination $file.name
        New-Item -ItemType Directory -Path (Split-Path -Parent $path) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $MaterialsDirectory ($kit.name + '/' + $file.name)) -Destination $path
    }
    Assert-Kit (Get-Tree $destination) $kit
}
$packageLines = @('# Native package originals', '', 'These are original package records and notices, not a final license classification. Missing package notices are shown explicitly; source supplements are separate.', '',
    'The old ZVBI package is excluded. See [scoped ZVBI materials](materials/zvbi-scoped-materials-v3/README.txt).', '',
    '| Package / version | Build / source recipe | Original notices |', '|---|---|---|')
$tar = Join-Path $env:SystemRoot 'System32/tar.exe'
foreach ($package in $packages) {
    $relative = 'packages/' + $package.name
    $directory = Join-Path $OutputDirectory $relative
    New-Item -ItemType Directory -Path $directory | Out-Null
    $inputRecord = $packageInputs[$package.name]
    # Only audited regular members of exact hash-pinned original packages.
    $members = @('.BUILDINFO','.PKGINFO') + @($package.package_notices.name)
    & $tar -xf $inputRecord.archive -C $directory @members
    if ($LASTEXITCODE -ne 0) { throw 'Native catalog package extraction failed.' }
    Assert-File (Join-Path $directory '.BUILDINFO') $package.buildinfo_sha256
    Assert-File (Join-Path $directory '.PKGINFO') $package.pkginfo_sha256
    foreach ($notice in $package.package_notices) { Assert-File (Join-Path $directory $notice.name) $notice.sha256 }
    Copy-Item -LiteralPath (Join-Path $RecipeDirectory $inputRecord.recipe.name) -Destination (Join-Path $directory 'PKGBUILD')
    Assert-File (Join-Path $directory 'PKGBUILD') $package.recipe_sha256
    $links = @($package.package_notices | ForEach-Object { '[' + (Split-Path -Leaf $_.name) + '](' + (Get-Link ($relative + '/' + $_.name)) + ')' })
    $notices = if ($links.Count) { $links -join ', ' } else { 'Not present in package; see source supplements.' }
    $packageLines += '| ' + $package.name + ' / ' + $package.version + ' | [build](' + (Get-Link ($relative + '/.BUILDINFO')) + '), [recipe](' + (Get-Link ($relative + '/PKGBUILD')) + '), [upstream recipe](' + $inputRecord.recipe.url + ') | ' + $notices + ' |'
}
$readme = @('# towavue application and native source/notice catalog', '', 'INCOMPLETE REVIEW MATERIALS - NOT AN APPROVED RELEASE', '',
    'Start with the component guide below. Native kits retain original source archives, notices and build inputs. The application kit adds its own licenses, Rust/font notices and the app-only MSVC Rust runtime notice ZIP. Each kit explains its provenance and scope; upstream license alternatives are not converted into combined requirements.', '',
    'The 71 original package owners and their available notices are listed in [PACKAGES.md](PACKAGES.md). The old ZVBI package is not copied into that list; the scoped replacement has its own materials. Baseline audit JSON inside older kits is historical evidence, not the adopted runtime list.', '',
    '## Component guide', '', '| Component | Contents and scope |', '|---|---|')
foreach ($kit in $inventory.kits) { $readme += '| [' + $kit.title + '](' + (Get-Link ('materials/' + $kit.name + '/README.txt')) + ') | ' + $kit.description + ' |' }
$readme += @('', '## Accessing source', '',
    'Open the relevant component directory. Its INPUTS.json (SCOPED-INPUTS.json and EVIDENCE.json for scoped ZVBI) records source/archive hashes and upstream locations. Source archives and their recorded patches remain together in that kit. Follow its README before attempting a build; this catalog does not execute recipes or fetch anything.', '',
    'This directory is an offline review copy, not a published download or source offer. The final approved application and required corresponding sources/notices must be made available through the same release delivery. No public endpoint is asserted here.', '',
    '## Work still required', '')
$readme += @($inventory.open_items | ForEach-Object { '- ' + $_ })
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'README.md'), ($readme -join "`n") + "`n", $utf8)
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'PACKAGES.md'), ($packageLines -join "`n") + "`n", $utf8)
Copy-Item -LiteralPath $auditPath -Destination (Join-Path $OutputDirectory 'native-runtime-package-audit.json')
Copy-Item -LiteralPath $recipesPath -Destination (Join-Path $OutputDirectory 'native-runtime-recipes.json')
$outputTree = Get-Tree $OutputDirectory
[IO.File]::WriteAllText((Join-Path $OutputDirectory 'FILES.json'), (ConvertTo-Json -InputObject $outputTree.files -Depth 5) + "`n", $utf8)
# Written last: interrupted or failed copies must not look complete.
Copy-Item -LiteralPath $inventoryPath -Destination (Join-Path $OutputDirectory 'CATALOG.json')
Write-Output "Native material catalog: $OutputDirectory"
Write-Output "$($inventory.kits.Count) exact kits, $($packages.Count) original packages, $($inventory.package_notice_count) notices; no runtime binaries copied or release authorized."

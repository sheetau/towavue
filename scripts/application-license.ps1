# Application licensing for current releases and retained evaluation inputs.
# Third-party licenses are selected by their own pinned inventories.
function Get-TowavueLicenseProfile([string]$ReleaseVersion) {
    if ($ReleaseVersion -and $ReleaseVersion -cnotmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\z') {
        throw 'Invalid application license release version.'
    }
    if ($ReleaseVersion -and [version]$ReleaseVersion -ge [version]'1.0.3') {
        return [pscustomobject]@{id='Apache-2.0';materials=@('LICENSE-APACHE','NOTICE')}
    }
    return [pscustomobject]@{id='MIT OR Apache-2.0';materials=@('LICENSE-MIT','LICENSE-APACHE')}
}

function Resolve-TowavueLicenseMaterial([string]$RepositoryRoot, [string]$Name) {
    if ($Name -eq 'LICENSE-MIT') {
        return Join-Path $RepositoryRoot 'third-party/towavue-legacy/LICENSE-MIT'
    }
    return Join-Path $RepositoryRoot $Name
}

function Assert-TowavueReleaseLicense([string]$RepositoryRoot, [string]$ReleaseVersion) {
    $cargo = Get-Content -LiteralPath (Join-Path $RepositoryRoot 'Cargo.toml') -Raw -Encoding UTF8
    $declarations = [regex]::Matches($cargo, '(?m)^license\s*=\s*"([^"]+)"\s*$')
    $profile = Get-TowavueLicenseProfile $ReleaseVersion
    if ($declarations.Count -ne 1 -or $declarations[0].Groups[1].Value -cne $profile.id) {
        throw 'Application license and release version differ. Apache-2.0-only development must ship as 1.0.3 or later.'
    }
}

function Get-TowavueLicenseLinks([string]$ReleaseVersion, [hashtable]$Links) {
    $profile = Get-TowavueLicenseProfile $ReleaseVersion
    foreach ($name in $profile.materials) {
        if (-not $Links.ContainsKey($name) -or -not $Links[$name]) {
            throw "Missing application license link: $name"
        }
    }
    $apache = '<a href="' + [Net.WebUtility]::HtmlEncode($Links['LICENSE-APACHE']) + '">Apache-2.0</a>'
    if ($profile.id -eq 'Apache-2.0') {
        return $apache + ' &middot; <a href="' + [Net.WebUtility]::HtmlEncode($Links['NOTICE']) + '">NOTICE</a>'
    }
    return '<a href="' + [Net.WebUtility]::HtmlEncode($Links['LICENSE-MIT']) + '">MIT</a> OR ' + $apache
}

[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('Rust', 'Materials', 'Installer')][string]$Phase,
    [string]$NsisArchive
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$powerShell = (Get-Process -Id $PID).Path
Set-Location -LiteralPath $repositoryRoot
$logDirectory = Join-Path $repositoryRoot ('target/tmp/ci-' + $Phase.ToLowerInvariant() + '-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $logDirectory | Out-Null
Write-Output "CI logs: $logDirectory"

# Keep local and hosted commands identical, including profile and concurrency.
# Each console child owns redirected pipes and cannot open a terminal window.
function Invoke-Check([string]$Name, [string]$File, [string]$Arguments) {
    Write-Output "Starting $Name"
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $File
    $info.Arguments = $Arguments
    $info.WorkingDirectory = $repositoryRoot
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $stdoutPath = Join-Path $logDirectory ($Name + '.out')
    $stderrPath = Join-Path $logDirectory ($Name + '.err')
    $stdoutFile = [IO.File]::Open($stdoutPath, 'CreateNew', 'Write', 'Read')
    $stderrFile = [IO.File]::Open($stderrPath, 'CreateNew', 'Write', 'Read')
    $process = $null
    $timer = [Diagnostics.Stopwatch]::StartNew()
    try {
        $process = [Diagnostics.Process]::Start($info)
        $stdout = $process.StandardOutput.BaseStream.CopyToAsync($stdoutFile)
        $stderr = $process.StandardError.BaseStream.CopyToAsync($stderrFile)
        while (-not $process.WaitForExit(30000)) {
            Write-Output ("{0} is running: PID {1}, elapsed {2:F0}s" -f $Name, $process.Id, $timer.Elapsed.TotalSeconds)
        }
        $stdout.GetAwaiter().GetResult() | Out-Null
        $stderr.GetAwaiter().GetResult() | Out-Null
        $exitCode = $process.ExitCode
    }
    finally {
        $stdoutFile.Dispose()
        $stderrFile.Dispose()
        if ($null -ne $process) { $process.Dispose() }
    }
    Get-Content -LiteralPath $stdoutPath -Encoding UTF8
    Get-Content -LiteralPath $stderrPath -Encoding UTF8
    Write-Output ("Finished {0}: exit={1}, seconds={2:F1}" -f $Name, $exitCode, $timer.Elapsed.TotalSeconds)
    if ($exitCode -ne 0) { throw "$Name failed; see $logDirectory" }
}

function Invoke-Script([string]$Name, [string]$Arguments = '') {
    Invoke-Check $Name $powerShell ("-NoProfile -ExecutionPolicy Bypass -File scripts/$Name.ps1 " + $Arguments)
}

if ($Phase -ne 'Installer') {
    $env:RUSTUP_TOOLCHAIN = '1.98.0'
    Invoke-Check 'toolchain' 'rustc' '--version'
    if ((Get-Content -LiteralPath (Join-Path $logDirectory 'toolchain.out') -Raw -Encoding UTF8) -notmatch '^rustc 1\.98\.0 ') {
        throw 'CI checks require the pinned Rust 1.98.0 toolchain.'
    }
}

switch ($Phase) {
    'Rust' {
        # Separate development FFmpeg artifacts from production packaging builds.
        $env:CARGO_TARGET_DIR = Join-Path $repositoryRoot 'target/ci-cargo'
        $env:CARGO_INCREMENTAL = '0'
        Invoke-Check 'format' 'cargo' 'fmt --all --check'
        $null = & (Join-Path $PSScriptRoot 'setup-ffmpeg.ps1')
        Invoke-Script 'generate-m1-fixtures'
        Invoke-Check 'renderer' 'cargo' 'test -p egui-directx11 --lib --locked -- --test-threads=4'
        Invoke-Check 'png' 'cargo' 'test --manifest-path vendor/png/Cargo.toml --lib row_filter_tests --features towavue-row-filter --locked -- --test-threads=4'
        Invoke-Check 'clippy' 'cargo' 'clippy --workspace --all-targets --locked -- -D warnings'
        Invoke-Check 'workspace' 'cargo' 'test --workspace --all-targets --locked --no-fail-fast -- --nocapture --test-threads=4'
    }
    'Materials' {
        Invoke-Check 'fetch' 'cargo' 'fetch --locked'
        Invoke-Script 'setup-rust-notices'
        Invoke-Script 'test-rust-notices'
        Invoke-Script 'prepare-rust-runtime-notices' '-Download'
        Invoke-Script 'test-rust-runtime-notices'
        Invoke-Script 'get-msys2-bootstrap' '-Download'
        Invoke-Script 'test-msys2-bootstrap'
        Invoke-Script 'test-vc-redist-status'
    }
    'Installer' {
        if (-not $NsisArchive) { throw 'Installer checks require -NsisArchive.' }
        $NsisArchive = (Resolve-Path -LiteralPath $NsisArchive).Path
        Invoke-Script 'test-setup-fixture' ('-NsisArchive "' + $NsisArchive + '"')
        Invoke-Script 'test-setup-update-lifecycle' ('-NsisArchive "' + $NsisArchive + '"')
        Invoke-Script 'test-candidate-material-archive'
        Invoke-Script 'test-setup-prerequisite' ('-NsisArchive "' + $NsisArchive + '"')
        Invoke-Script 'test-setup-registration'
        Invoke-Script 'test-setup-update-plan'
        Invoke-Script 'test-setup-update-transaction'
    }
}

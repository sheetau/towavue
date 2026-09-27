[CmdletBinding()]
param([Parameter(Mandatory)][string]$NsisArchive)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$pins = Get-Content -LiteralPath (Join-Path $repositoryRoot 'docs/nsis-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$NsisArchive = (Resolve-Path -LiteralPath $NsisArchive).Path
if ((Get-Item -LiteralPath $NsisArchive).Length -ne $pins.archive.bytes -or (Get-FileHash -LiteralPath $NsisArchive).Hash -ne $pins.archive.sha256) {
    throw 'NSIS archive identity mismatch.'
}
$root = Join-Path $repositoryRoot ('target/tmp/setup-prerequisite-flow-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
Add-Type -AssemblyName System.IO.Compression.FileSystem
[IO.Compression.ZipFile]::ExtractToDirectory($NsisArchive, (Join-Path $root 'toolchain'))
$compiler = Join-Path $root 'toolchain/nsis-3.12/makensis.exe'
$utf8 = [Text.UTF8Encoding]::new($false)
$source = Get-Content -LiteralPath (Join-Path $repositoryRoot 'packaging/windows/setup.nsi') -Raw -Encoding UTF8
$functions = [regex]::Matches($source, '(?ms)^Function CheckPrerequisite\r?\n.*?^FunctionEnd')
if ($functions.Count -ne 1) { throw 'Missing production prerequisite function.' }
$function = $functions[0].Value
if ($function -match '(?m)^\s*MessageBox\b') { throw 'Prerequisite flow must use Setup status/Finish instead of an extra dialog.' }

function Write-Fixture([string]$Path, [string]$Text) {
    New-Item -ItemType Directory -Path (Split-Path -Parent $Path) -Force | Out-Null
    [IO.File]::WriteAllText($Path, $Text, $utf8)
}
function Invoke-Fixture([string]$File, [string]$Arguments, [int]$Expected) {
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $File
    $info.Arguments = $Arguments
    $info.WorkingDirectory = $root
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $info.RedirectStandardOutput = $true
    $info.RedirectStandardError = $true
    $process = [Diagnostics.Process]::Start($info)
    $stdout = $process.StandardOutput.ReadToEndAsync()
    $stderr = $process.StandardError.ReadToEndAsync()
    if (-not $process.WaitForExit(30000)) { throw "Owned prerequisite fixture remains live: PID $($process.Id). Do not restart." }
    try {
        if ($process.ExitCode -ne $Expected) { throw "Fixture exit $($process.ExitCode), expected ${Expected}: $($stdout.Result) $($stderr.Result)" }
    } finally { $process.Dispose() }
}
# Compile the unchanged production NSIS function. Only its extracted helper is
# synthetic; the embedded package is inert text. No VC install, UAC, registry or
# application files are touched, even when testing the automatic-update branch.
$helper = @'
param([string]$Mode, [string]$PackagePath)
$case = Get-Content -LiteralPath (Join-Path $PSScriptRoot '../docs/vc-redist-inputs.json') -Raw -Encoding UTF8 | ConvertFrom-Json
[IO.File]::AppendAllText($case.transcript, $Mode + "`n")
if ($Mode -eq 'Inspect') { exit $case.inspect }
if ($Mode -eq 'Install') { exit $case.install }
exit 99
'@
$template = @'
Unicode true
RequestExecutionLevel user
SilentInstall silent
!include "LogicLib.nsh"
!define TRIAL_ROOT "@ROOT@"
OutFile "${TRIAL_ROOT}\flow.exe"
Var PrerequisiteResult
Var AutomaticUpdate
Section
  StrCpy $AutomaticUpdate @AUTOMATIC@
  Call CheckPrerequisite
  FileOpen $0 "${TRIAL_ROOT}\completed.txt" w
  FileWrite $0 "$PrerequisiteResult"
  FileClose $0
  SetErrorLevel $PrerequisiteResult
SectionEnd
'@
$cases = @(
    @{name='satisfied';automatic=0;inspect=0;install=99;expected=0;calls="Inspect`n"},
    @{name='required';automatic=0;inspect=10;install=0;expected=0;calls="Inspect`nInstall`n"},
    @{name='automatic required';automatic=1;inspect=10;install=0;expected=0;calls="Inspect`nInstall`n"},
    @{name='restart';automatic=0;inspect=10;install=3010;expected=3010;calls="Inspect`nInstall`n"},
    @{name='automatic restart';automatic=1;inspect=10;install=3010;expected=3010;calls="Inspect`nInstall`n"},
    @{name='install failure';automatic=0;inspect=10;install=20;expected=3;calls="Inspect`nInstall`n"},
    @{name='unknown state';automatic=1;inspect=20;install=99;expected=3;calls="Inspect`n"}
)
foreach ($case in $cases) {
    $directory = Join-Path $root $case.name
    $transcript = Join-Path $directory 'calls.txt'
    Write-Fixture (Join-Path $directory 'prerequisite/docs/vc-redist-inputs.json') (@{inspect=$case.inspect;install=$case.install;transcript=$transcript} | ConvertTo-Json)
    Write-Fixture (Join-Path $directory 'prerequisite/scripts/prerequisite.ps1') $helper
    foreach ($name in @('get-vc-redist-status.ps1', 'vc-redist-state.ps1')) {
        Write-Fixture (Join-Path $directory "prerequisite/scripts/$name") '# Inert fixture; not invoked.'
    }
    Write-Fixture (Join-Path $directory 'prerequisite/vc_redist.x64.exe') 'Inert fixture; never executed.'
    $program = $template.Replace('@ROOT@', $directory).Replace('@AUTOMATIC@', [string]$case.automatic) + "`n" + $function + "`n"
    $script = Join-Path $directory 'flow.nsi'
    Write-Fixture $script $program
    Invoke-Fixture $compiler ('/NOCONFIG /V2 "' + $script + '"') 0
    Invoke-Fixture (Join-Path $directory 'flow.exe') '/S' $case.expected
    if ([IO.File]::ReadAllText($transcript) -cne $case.calls) { throw "Wrong helper sequence: $($case.name)" }
    $completed = Join-Path $directory 'completed.txt'
    if ($case.expected -eq 3) {
        if (Test-Path -LiteralPath $completed) { throw "Setup continued after failure: $($case.name)" }
    } elseif ([IO.File]::ReadAllText($completed) -cne [string]$case.expected) {
        throw "Setup lost prerequisite result: $($case.name)"
    }
}
Write-Output "PASS: $($cases.Count) native NSIS prerequisite flows, skip/install/error/restart and automatic-update handoff. No real VC package, UAC or visible UI. Evidence: $root"

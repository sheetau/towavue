[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'release-signing.ps1')
$repository = Split-Path -Parent $PSScriptRoot
$root = Join-Path $repository ('target/tmp/update-handoff-' + [Guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($root) | Out-Null
$keyPath = Join-Path $root 'test-key.dpapi'
$publicPath = Join-Path $root 'test-public.hex'
New-TowavueSigningKey $keyPath $publicPath
$publicHex = [IO.File]::ReadAllText($publicPath,[Text.Encoding]::UTF8).Trim()
$originalHelper = [IO.File]::ReadAllText((Join-Path $repository 'crates/towavue-runtime-windows/src/update/handoff.cs'),[Text.Encoding]::UTF8)
$utf8 = [Text.UTF8Encoding]::new($false)
$systemPowerShell = Join-Path ([Environment]::SystemDirectory) 'WindowsPowerShell/v1.0/powershell.exe'
$results = [Collections.Generic.List[object]]::new()

function Assert-True([bool]$Condition,[string]$Message) { if (-not $Condition) { throw $Message } }
function Write-Text([string]$Path,[string]$Text) { [IO.File]::WriteAllText($Path,$Text,$utf8) }
function Quoted([string]$Value) {
    # These normalized test paths cannot contain a double quote or a trailing slash.
    if ($Value.Contains('"') -or $Value.EndsWith('\')) { throw 'Invalid test argument.' }
    '"' + $Value + '"'
}
function Wait-File([string]$Path,$Process) {
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $Path)) {
        if ($Process.HasExited) { throw "Child exited before $Path (code $($Process.ExitCode))." }
        if ([DateTime]::UtcNow -gt $deadline) { throw "Timed out waiting for $Path." }
        Start-Sleep -Milliseconds 50
    }
}
function Wait-Child($Process) {
    $null = $Process.Handle
    if (-not $Process.WaitForExit(30000)) { throw 'Fixture child timed out.' }
    $Process.Refresh()
    $Process.ExitCode
}

foreach ($case in @('manifest','payload','parent','junction','success','missing-media','setup-failure','reboot','interrupted')) {
    $caseRoot = Join-Path $root $case
    $cache = Join-Path $caseRoot 'cache'
    $stage = 'stage-' + ('a' * 56)
    $attempt = 'stage-' + ('b' * 56)
    $stagePath = Join-Path $cache $stage
    $installation = Join-Path $caseRoot "installed with spaces & 'quote' 日本語"
    $media = Join-Path $caseRoot "media & 'quote' 日本語.txt"
    $keyName = 'Software\towavue\InstallerTests\' + [Guid]::NewGuid().ToString('N')
    $parent = $null; $helper = $null; $base = $null
    try {
        [IO.Directory]::CreateDirectory($stagePath) | Out-Null
        [IO.Directory]::CreateDirectory($installation) | Out-Null
        Write-Text $media 'fixture media'
        $appSource = @'
using System;
using System.IO;
using System.Reflection;
[assembly: AssemblyVersion("@VERSION@")]
[assembly: AssemblyFileVersion("@VERSION@")]
[assembly: AssemblyInformationalVersion("@VERSION@")]
public static class FixtureApp {
    public static void Main(string[] args) {
        string root = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
        string release = Path.Combine(root, "parent.release");
        if (File.Exists(release)) { File.WriteAllText(Path.Combine(root,"restarted.arguments"),String.Join("\n",args)); return; }
        File.WriteAllText(Path.Combine(root,"parent.started"),"started");
        DateTime deadline = DateTime.UtcNow.AddSeconds(90);
        while (!File.Exists(release) && DateTime.UtcNow < deadline) System.Threading.Thread.Sleep(50);
    }
}
'@
        $app = Join-Path $installation 'towavue.exe'
        $incoming = Join-Path $stagePath 'incoming.exe'
        Add-Type -TypeDefinition ($appSource.Replace('@VERSION@','1.0.0').Replace('FixtureApp','FixtureOld' + [Guid]::NewGuid().ToString('N'))) -OutputAssembly $app -OutputType WindowsApplication
        Add-Type -TypeDefinition ($appSource.Replace('@VERSION@','1.0.1').Replace('FixtureApp','FixtureNew' + [Guid]::NewGuid().ToString('N'))) -OutputAssembly $incoming -OutputType WindowsApplication
        $setupSource = @'
using System;
using System.IO;
using System.Reflection;
using Microsoft.Win32;
public static class FixtureSetup {
    public static int Main() {
        string root = Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location);
        string command = Environment.CommandLine;
        File.WriteAllText(Path.Combine(root,"setup.called"),command);
        string prefix = "/S /TOWAVUEUPDATE=1 /D=";
        int at = command.IndexOf(prefix,StringComparison.Ordinal);
        if (at < 0) return 90;
        string destination = command.Substring(at + prefix.Length);
        if (destination != @"@INSTALLATION@") return 91;
        if (@FAIL@) return @EXIT@;
        File.Copy(Path.Combine(root,"incoming.exe"),Path.Combine(destination,"towavue.exe"),true);
        using (var user = RegistryKey.OpenBaseKey(RegistryHive.CurrentUser,RegistryView.Registry64))
        using (var key = user.OpenSubKey(@"@REGISTRY@",true)) key.SetValue("DisplayVersion","1.0.1",RegistryValueKind.String);
        return 0;
    }
}
'@
        $setup = Join-Path $stagePath 'setup.exe'
        $source = $setupSource.Replace('@INSTALLATION@',$installation.Replace('"','""')).Replace('@REGISTRY@',$keyName).Replace('@FAIL@',$(if ($case -in @('setup-failure','reboot')) { 'DateTime.UtcNow.Year > 2000' } else { 'DateTime.UtcNow.Year < 2000' })).Replace('@EXIT@',$(if ($case -eq 'reboot') { '3010' } else { '20' })).Replace('FixtureSetup','FixtureSetup' + [Guid]::NewGuid().ToString('N'))
        Add-Type -TypeDefinition $source -OutputAssembly $setup -OutputType WindowsApplication
        $hash = (Get-FileHash -LiteralPath $setup).Hash.ToLowerInvariant()
        $length = (Get-Item -LiteralPath $setup).Length
        $manifest = Join-Path $stagePath 'manifest.txt'
        Write-Text $manifest "towavue-update-v1`n1.0.1`nwindows-x64`n$length`n$hash`n"
        Write-TowavueUpdateSignature $manifest (Join-Path $stagePath 'manifest.sig') $keyPath $publicPath
        Write-Text (Join-Path $cache 'state.txt') "towavue-update-state-v1`n$stage`ninstalling`n"
        $helperSource = $originalHelper.Replace('@PUBLIC_KEY_HEX@',$publicHex).Replace('Software\Microsoft\Windows\CurrentVersion\Uninstall\towavue',$keyName)
        $script = Join-Path $stagePath 'helper.ps1'
        Write-Text $script ("param([string]`$CacheRoot,[string]`$Stage,[string]`$Attempt,[string]`$Installation,[int]`$ParentId,[long]`$ParentStart,[string]`$InitialPath)`n`$ErrorActionPreference='Stop'`nAdd-Type -TypeDefinition @'`n" + $helperSource + "`n'@`nexit ([TowavueReleaseHandoff]::Run(`$CacheRoot,`$Stage,`$Attempt,`$Installation,`$ParentId,`$ParentStart,`$InitialPath))`n")
        $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser,[Microsoft.Win32.RegistryView]::Registry64)
        $entry = $base.CreateSubKey($keyName)
        try { $entry.SetValue('InstallLocation',$installation); $entry.SetValue('DisplayVersion','1.0.0') } finally { $entry.Dispose() }
        $parent = Start-Process -FilePath $app -WindowStyle Hidden -PassThru
        $null = $parent.Handle
        Wait-File (Join-Path $installation 'parent.started') $parent
        $parentStart = $parent.StartTime.ToUniversalTime().ToFileTimeUtc()
        switch ($case) {
            'parent' { $parentStart += 1 }
            'manifest' { Write-Text $manifest ([IO.File]::ReadAllText($manifest).Replace('1.0.1','1.0.2')) }
            'payload' {
                $payload = [IO.File]::ReadAllBytes($setup); $payload[$payload.Length-1] = $payload[$payload.Length-1] -bxor 1
                [IO.File]::WriteAllBytes($setup,$payload)
            }
            'junction' {
                $link = Join-Path $caseRoot 'cache-junction'
                New-Item -ItemType Junction -Path $link -Target $cache | Out-Null
                $cache = $link
            }
        }
        $arguments = '-NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File ' + (Quoted $script) + ' -CacheRoot ' + (Quoted $cache) + ' -Stage ' + $stage + ' -Attempt ' + $attempt + ' -Installation ' + (Quoted $installation) + ' -ParentId ' + $parent.Id + ' -ParentStart ' + $parentStart + ' -InitialPath ' + (Quoted $media)
        $helper = Start-Process -FilePath $systemPowerShell -ArgumentList $arguments -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $caseRoot 'helper.stdout') -RedirectStandardError (Join-Path $caseRoot 'helper.stderr')
        $null = $helper.Handle
        if ($case -in @('success','missing-media','setup-failure','reboot','interrupted')) {
            Wait-File (Join-Path $stagePath ($attempt + '.ready')) $helper
            Assert-True (-not (Test-Path -LiteralPath (Join-Path $stagePath 'setup.called'))) 'Setup started before the parent exited.'
            $refused = $false
            try { $changed = [IO.File]::Open($setup,[IO.FileMode]::Open,[IO.FileAccess]::Write,[IO.FileShare]::ReadWrite); $changed.Dispose() } catch { $refused = $true }
            Assert-True $refused 'The authenticated Setup was not protected from writes.'
            if ($case -eq 'interrupted') {
                $helper.Kill(); $helper.WaitForExit()
                Assert-True (-not (Test-Path -LiteralPath (Join-Path $stagePath 'setup.called'))) 'Interrupted handoff executed Setup.'
                Assert-True ([IO.File]::ReadAllText((Join-Path $cache 'state.txt')).EndsWith("installing`n")) 'Interrupted state should be recovered by the next primary host.'
                $results.Add([pscustomobject]@{case=$case;status='passed'})
                continue
            }
            $competing = Start-Process -FilePath $systemPowerShell -ArgumentList $arguments -WindowStyle Hidden -PassThru
            try { Assert-True ((Wait-Child $competing) -eq 21) 'A competing helper was accepted.' } finally { $competing.Dispose() }
            Assert-True ([IO.File]::ReadAllText((Join-Path $cache 'state.txt')).EndsWith("installing`n")) 'Competitor changed the active state.'
            if ($case -eq 'missing-media') { [IO.File]::Delete($media) }
            Write-Text (Join-Path $installation 'parent.release') 'exit now'
            $code = Wait-Child $helper
            Assert-True ((Wait-Child $parent) -eq 0) 'Fixture parent failed.'
            if ($case -eq 'reboot') {
                Assert-True (-not (Test-Path -LiteralPath (Join-Path $installation 'restarted.arguments'))) 'Reboot-required Setup must not restart the application.'
            } else {
                $deadline = [DateTime]::UtcNow.AddSeconds(10)
                while (-not (Test-Path -LiteralPath (Join-Path $installation 'restarted.arguments')) -and [DateTime]::UtcNow -lt $deadline) { Start-Sleep -Milliseconds 50 }
                $expectedArgument = if ($case -eq 'missing-media') { '' } else { $media }
                Assert-True ([IO.File]::ReadAllText((Join-Path $installation 'restarted.arguments')) -ceq $expectedArgument) 'Restart did not preserve the expected media argument.'
            }
            if ($case -in @('success','missing-media')) {
                Assert-True ($code -eq 0) 'Positive handoff failed.'
                Assert-True ((Get-Item -LiteralPath $app).VersionInfo.ProductVersion -eq '1.0.1') 'Incoming executable version was not installed.'
                $entry = $base.OpenSubKey($keyName)
                try { Assert-True ($entry.GetValue('DisplayVersion') -eq '1.0.1') 'Incoming registry version was not installed.' } finally { $entry.Dispose() }
            } else { Assert-True ($code -eq 20) 'Failed Setup was reported as success.' }
        } else {
            $code = Wait-Child $helper
            $expectedCode = if ($case -eq 'junction') { 21 } else { 20 }
            Assert-True ($code -eq $expectedCode) "Expected refusal: $case; actual exit: $code"
            Assert-True (-not (Test-Path -LiteralPath (Join-Path $stagePath 'setup.called'))) 'Invalid update executed.'
        }
        if ($case -notin @('success','missing-media','junction')) { Assert-True ([IO.File]::ReadAllText((Join-Path $cache 'state.txt')).EndsWith("failed`n")) 'Failure was not retained for one-shot recovery.' }
        $results.Add([pscustomobject]@{case=$case;status='passed'})
    } finally {
        foreach ($process in @($helper,$parent)) { if ($process) { if (-not $process.HasExited) { $process.Kill(); $process.WaitForExit() }; $process.Dispose() } }
        if ($base) { $base.DeleteSubKeyTree($keyName,$false); $base.Dispose() }
    }
}
Write-Text (Join-Path $root 'results.json') ($results | ConvertTo-Json)
Write-Output "PASS: isolated helper signature/payload/parent/junction refusal, exact-process wait, locked Setup, competing claim, real fixture version replacement, failed/reboot-required/interrupted Setup and Unicode restart argument. Evidence: $root"

$ErrorActionPreference = 'Stop'
$dashboardRoot = $PSScriptRoot
$manifest = Join-Path $dashboardRoot 'runtime.json'
$database = [IO.Path]::GetFullPath((Join-Path $dashboardRoot 'state.sqlite'))
if (-not (Test-Path -LiteralPath $database)) { throw 'Run python .dashboard/app.py init first, or restore your private backup.' }
$dashboardUrl = $null
if (Test-Path -LiteralPath $manifest) {
    try {
        $state = Get-Content -LiteralPath $manifest -Encoding UTF8 -Raw | ConvertFrom-Json
        if ($state.url -match '^http://127\.0\.0\.1:\d+$') {
            $health = Invoke-RestMethod -Uri ($state.url + '/health') -TimeoutSec 2
            if ($health.service -eq 'project-dashboard' -and $health.database -eq $database) { $dashboardUrl = $state.url }
        }
    } catch { }
}
if (-not $dashboardUrl) {
    $python = (Get-Command python.exe -ErrorAction Stop).Source
    $info = [Diagnostics.ProcessStartInfo]::new()
    $info.FileName = $python
    $info.Arguments = '"' + (Join-Path $dashboardRoot 'app.py') + '" serve'
    $info.WorkingDirectory = $dashboardRoot
    $info.UseShellExecute = $false
    $info.CreateNoWindow = $true
    $child = [Diagnostics.Process]::Start($info)
    for ($attempt = 0; $attempt -lt 50; $attempt++) {
        Start-Sleep -Milliseconds 100
        if ($child.HasExited) { throw 'Dashboard failed to start. Run app.py serve to read the error.' }
        if (Test-Path -LiteralPath $manifest) {
            try {
                $state = Get-Content -LiteralPath $manifest -Encoding UTF8 -Raw | ConvertFrom-Json
                if ($state.pid -eq $child.Id) { $dashboardUrl = $state.url; break }
            } catch { }
        }
    }
    $child.Dispose()
    if (-not $dashboardUrl) { throw 'Dashboard startup timed out.' }
}
Start-Process $dashboardUrl

[CmdletBinding()]
param([Parameter(Mandatory = $true)][string]$MsysRoot)

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$MsysRoot = (Resolve-Path -LiteralPath $MsysRoot).Path
$inspector = Join-Path $PSScriptRoot 'get-native-runtime-dependencies.ps1'
$output = Join-Path $repositoryRoot ('target/tmp/runtime-dependencies-' + [guid]::NewGuid().ToString('N'))
$bin = Join-Path $MsysRoot 'mingw64/bin'
$objdump = Join-Path $bin 'objdump.exe'
$savedPath = $env:PATH
try {
    $env:PATH = "$bin;" + (Join-Path $env:SystemRoot 'System32')
    & "$bin/cmake.exe" -S (Join-Path $PSScriptRoot 'fixtures/msys2-smoke') -B "$output/build" -G Ninja `
        "-DCMAKE_C_COMPILER=$bin/gcc.exe" "-DCMAKE_CXX_COMPILER=$bin/g++.exe" "-DCMAKE_MAKE_PROGRAM=$bin/ninja.exe"
    if ($LASTEXITCODE -ne 0) { throw 'Dependency fixture configuration failed.' }
    & "$bin/cmake.exe" --build "$output/build"
    if ($LASTEXITCODE -ne 0) { throw 'Dependency fixture build failed.' }
    $entry = "$output/build/smoke_test.exe"
    Push-Location $output
    try { $graph = @(& $inspector -EntryPoints $entry -SearchDirectories "$output/build",$bin -ObjdumpExecutable $objdump) }
    finally { Pop-Location }
    foreach ($required in @('smoke_test.exe', 'libsmoke.dll', 'libstdc++-6.dll', 'libwinpthread-1.dll')) {
        if ($required -notin $graph.name) { throw "Missing fixture dependency: $required" }
    }
    foreach ($record in $graph) {
        if ((Get-FileHash -LiteralPath $record.path).Hash -ne $record.sha256) { throw 'Dependency hash changed during read-only inspection.' }
    }
    foreach ($case in @('missing', 'duplicate', 'system-shadow')) {
        $extra = "$output/$case"
        New-Item -ItemType Directory -Path $extra | Out-Null
        $directories = @("$output/build", $bin, $extra)
        if ($case -eq 'missing') { $directories = @($bin) }
        elseif ($case -eq 'duplicate') { Copy-Item -LiteralPath "$output/build/libsmoke.dll" -Destination "$extra/libsmoke.dll" }
        else { Copy-Item -LiteralPath "$output/build/libsmoke.dll" -Destination "$extra/KERNEL32.dll" }
        $rejected = $false
        try { & $inspector -EntryPoints $entry -SearchDirectories $directories -ObjdumpExecutable $objdump | Out-Null }
        catch {
            $expected = if ($case -eq 'system-shadow') { 'Local file conflicts with a system DLL:*' } else { 'Expected exactly one allowed location for libsmoke.dll*' }
            if ($_.Exception.Message -notlike $expected) { throw }
            $rejected = $true
        }
        if (-not $rejected) { throw "Dependency inspector accepted $case input." }
    }
    Write-Output 'PE dependency graph, hashes, arbitrary cwd, missing/duplicate DLL and KnownDLL shadow rejection passed.'
    Write-Output "Artifacts: $output"
    Write-Output 'Dynamic loading, target-OS availability and redistribution approval are not covered.'
}
finally { $env:PATH = $savedPath }

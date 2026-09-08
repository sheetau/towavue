[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$ReferenceExecutable,
    [Parameter(Mandatory = $true)][string]$CandidateExecutable
)

$ErrorActionPreference = 'Stop'
$ReferenceExecutable = (Resolve-Path -LiteralPath $ReferenceExecutable).Path
$CandidateExecutable = (Resolve-Path -LiteralPath $CandidateExecutable).Path
if ($ReferenceExecutable -eq $CandidateExecutable) { throw 'Use distinct reference and candidate executables.' }
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$output = Join-Path $repositoryRoot ('target/tmp/chromaprint-comparison-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $output | Out-Null
$savedPath = $env:PATH
try {
    # Each executable must resolve its own adjacent runtime, not development PATH entries.
    $env:PATH = Join-Path $env:SystemRoot 'System32'
    foreach ($sampleRate in @(11025, 44100)) {
        $channels = if ($sampleRate -eq 11025) { 1 } else { 2 }
        $signals = [ordered]@{
            tone = "sine=frequency=440:sample_rate=${sampleRate}:duration=20"
            chirp = "aevalsrc=0.5*sin(2*PI*(200*t+40*t*t)):s=${sampleRate}:d=20"
            noise = "anoisesrc=sample_rate=${sampleRate}:duration=20:color=white:seed=1202"
            silence = "anullsrc=sample_rate=${sampleRate}:channel_layout=mono:d=20"
        }
        foreach ($name in $signals.Keys) {
            $stem = Join-Path $output "$name-$sampleRate-$channels"
            & $ReferenceExecutable -nostdin -v error -n -f lavfi -i $signals[$name] `
                -ac $channels -c:a pcm_s16le -f s16le "$stem.pcm"
            if ($LASTEXITCODE -ne 0) { throw "Cannot generate PCM fixture: $stem" }
            $expectedBytes = 20 * $sampleRate * $channels * 2
            if ((Get-Item -LiteralPath "$stem.pcm").Length -ne $expectedBytes) { throw 'Unexpected PCM duration or format.' }
            foreach ($variant in @('reference', 'candidate')) {
                $executable = if ($variant -eq 'reference') { $ReferenceExecutable } else { $CandidateExecutable }
                & $executable -nostdin -v error -n -f s16le -ar $sampleRate -ac $channels -i "$stem.pcm" `
                    -c:a pcm_s16le -f chromaprint -algorithm 1 -fp_format raw "$stem.$variant.raw"
                if ($LASTEXITCODE -ne 0) { throw "Chromaprint failed: $variant / $stem" }
                if ((Get-Item -LiteralPath "$stem.$variant.raw").Length -ne 560) {
                    throw "Expected 140 fingerprint words: $variant / $stem"
                }
            }
            $referenceHash = (Get-FileHash -LiteralPath "$stem.reference.raw" -Algorithm SHA256).Hash
            $candidateHash = (Get-FileHash -LiteralPath "$stem.candidate.raw" -Algorithm SHA256).Hash
            if ($referenceHash -ne $candidateHash) { throw "Fingerprint mismatch: $stem" }
            Write-Output "PASS $name / $sampleRate Hz / $channels channels: 140 identical fingerprint words."
        }
    }
    Write-Output "Retained comparison fixtures: $output"
    Write-Output 'This checks eight generated signals, not universal equivalence, backend provenance or performance.'
}
finally {
    $env:PATH = $savedPath
}

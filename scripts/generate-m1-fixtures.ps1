[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$ffmpegRoot = & (Join-Path $PSScriptRoot 'setup-ffmpeg.ps1')
$ffmpeg = Join-Path $ffmpegRoot 'bin\ffmpeg.exe'
$ffprobe = Join-Path $ffmpegRoot 'bin\ffprobe.exe'
$outputDirectory = Join-Path $repositoryRoot 'tests\generated\m1'
$mp4 = Join-Path $outputDirectory 'h264-aac.mp4'
$mkv = Join-Path $outputDirectory 'hevc-aac.mkv'
$webm = Join-Path $outputDirectory 'vp9-opus.webm'

New-Item -ItemType Directory -Path $outputDirectory -Force | Out-Null

function New-Fixture {
    param(
        [string]$Path,
        [string]$VideoCodec,
        [string]$AudioCodec,
        [string[]]$VideoOptions
    )

    $arguments = @(
        '-hide_banner', '-loglevel', 'error', '-y',
        '-f', 'lavfi', '-i', 'testsrc2=size=160x96:rate=30:duration=2',
        '-f', 'lavfi', '-i', 'sine=frequency=440:sample_rate=48000:duration=2',
        '-map', '0:v:0', '-map', '1:a:0',
        '-c:v', $VideoCodec, '-pix_fmt', 'yuv420p'
    )
    $arguments += $VideoOptions
    $arguments += @('-c:a', $AudioCodec, '-shortest', $Path)
    & $ffmpeg $arguments
    if ($LASTEXITCODE -ne 0) {
        throw "FFmpeg failed to generate $Path."
    }
}

function Assert-Codec {
    param(
        [string]$Path,
        [string]$ExpectedVideo,
        [string]$ExpectedAudio
    )

    $videoCodec = & $ffprobe -v error -select_streams v:0 -show_entries stream=codec_name -of default=nw=1:nk=1 $Path
    $audioCodec = & $ffprobe -v error -select_streams a:0 -show_entries stream=codec_name -of default=nw=1:nk=1 $Path
    if ($videoCodec.Trim() -ne $ExpectedVideo -or $audioCodec.Trim() -ne $ExpectedAudio) {
        throw "Unexpected codecs in ${Path}: video=$videoCodec audio=$audioCodec."
    }
}

New-Fixture -Path $mp4 -VideoCodec 'libopenh264' -AudioCodec 'aac' -VideoOptions @('-b:v', '250k')
New-Fixture -Path $mkv -VideoCodec 'libkvazaar' -AudioCodec 'aac' -VideoOptions @('-preset', 'ultrafast', '-b:v', '250k')
New-Fixture -Path $webm -VideoCodec 'libvpx-vp9' -AudioCodec 'libopus' -VideoOptions @('-deadline', 'realtime', '-cpu-used', '8', '-b:v', '250k')

Assert-Codec -Path $mp4 -ExpectedVideo 'h264' -ExpectedAudio 'aac'
Assert-Codec -Path $mkv -ExpectedVideo 'hevc' -ExpectedAudio 'aac'
Assert-Codec -Path $webm -ExpectedVideo 'vp9' -ExpectedAudio 'opus'

Write-Output $outputDirectory

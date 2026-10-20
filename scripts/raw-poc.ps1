[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$source = Join-Path $projectRoot 'tests/fixtures/generated/p1/clip-mp4-fr.mp4'
$scratch = Join-Path $projectRoot ('target/raw-poc-' + [Guid]::NewGuid().ToString('N'))
$reportDir = Join-Path $projectRoot 'docs/evidence/2026-10-18/raw-poc'
New-Item -ItemType Directory -Path $scratch, $reportDir -Force | Out-Null
$video = Join-Path $scratch 'video.obu'
$audio = Join-Path $scratch 'audio.packets'
& ffmpeg -v error -nostdin -i $source -map 0:v:0 -c copy -f obu $video
if ($LASTEXITCODE -ne 0) { throw 'Synthetic AV1 raw extraction failed.' }
& ffmpeg -v error -nostdin -i $source -map 0:a:0 -c copy -f data $audio
if ($LASTEXITCODE -ne 0) { throw 'Synthetic Opus packet extraction failed.' }
$original = (& ffprobe -v error -show_streams -show_packets -of json $source | Out-String | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0) { throw 'Reference probe failed.' }
$rawVideo = (& ffprobe -v error -f obu -show_streams -show_packets -of json $video | Out-String | ConvertFrom-Json)
if ($LASTEXITCODE -ne 0) { throw 'Raw AV1 probe failed.' }
& ffprobe -v error -show_streams -show_packets -of json $audio 1> (Join-Path $scratch 'audio-probe.json') 2> (Join-Path $scratch 'audio-probe-error.txt')
$audioExit = $LASTEXITCODE
$videoStream = @($original.streams | Where-Object codec_type -eq 'video')[0]
$audioStream = @($original.streams | Where-Object codec_type -eq 'audio')[0]
$report = [ordered]@{
    fixture = 'generated/p1/clip-mp4-fr.mp4'
    method = 'FFmpeg stream copy to AV1 OBU and concatenated Opus payloads; originals read only'
    referenceVideoTimebase = $videoStream.time_base
    referenceAudioTimebase = $audioStream.time_base
    referenceAudioSampleRate = $audioStream.sample_rate
    referenceAudioChannels = $audioStream.channels
    referenceAudioInitBytes = $audioStream.extradata_size
    rawVideoPacketCount = @($rawVideo.packets).Count
    rawVideoPacketsWithoutPts = @($rawVideo.packets | Where-Object { $null -eq $_.pts }).Count
    referenceVideoDuration = $videoStream.duration
    rawVideoTimebase = $rawVideo.streams[0].time_base
    rawVideoLastPtsSeconds = $rawVideo.packets[-1].pts_time
    referenceVideoLastPtsSeconds = @($original.packets | Where-Object stream_index -eq $videoStream.index)[-1].pts_time
    rawOpusProbeExitCode = $audioExit
    rawOpusBytes = (Get-Item -LiteralPath $audio).Length
    integrated = $false
    conclusion = 'No demonstrated synchronized raw playback: AV1 timestamps and Opus packet boundaries/init/pre-skip must be retained in an explicit index. No production raw support claimed.'
}
$report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $reportDir 'report.json') -Encoding utf8
$report | ConvertTo-Json -Depth 6

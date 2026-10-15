param(
    [string]$SourceDirectory = 'tests/fixtures/generated',
    [string]$OutputDirectory = 'tests/fixtures/generated/language-remux'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
# Offline P0C preparation only: never rewrite the original, never overwrite outputs.
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Output directory already exists. Choose a new output path.' }
$sourceRoot = (Resolve-Path -LiteralPath $SourceDirectory).Path
$originalNames = @('av1-opus.mkv', 'av1-opus.mp4', 'av1-opus.webm', 'av1-opus-fragmented.mp4')
$before = @{}
foreach ($name in $originalNames) {
    $before[$name] = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $sourceRoot $name)).Hash
}
$commands = [System.Collections.Generic.List[object]]::new()
function Invoke-MediaTool([string]$Tool, [string[]]$Arguments) {
    $timer = [System.Diagnostics.Stopwatch]::StartNew()
    $output = @(& $Tool @Arguments)
    $exitCode = $LASTEXITCODE
    $timer.Stop()
    $commands.Add(@{tool = $Tool; arguments = $Arguments; elapsed_ms = $timer.Elapsed.TotalMilliseconds; exit_code = $exitCode})
    if ($exitCode -ne 0) { throw "$Tool failed: $exitCode" }
    return $output
}
function Get-Probe([string]$Path) {
    $raw = Invoke-MediaTool 'ffprobe' @('-v','error','-show_format','-show_streams','-show_data','-of','json',$Path)
    return (($raw -join "`n") | ConvertFrom-Json)
}
function Get-PacketHashes([string]$Path) {
    $raw = Invoke-MediaTool 'ffprobe' @('-v','error','-show_packets','-show_data_hash','sha256','-show_entries','packet=stream_index,data_hash','-of','json',$Path)
    return (($raw -join "`n") | ConvertFrom-Json).packets
}
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$outputRoot = (Resolve-Path -LiteralPath $OutputDirectory).Path
$totalTimer = [System.Diagnostics.Stopwatch]::StartNew()
$sourceProbes = @{}
$sourcePackets = @{}
$results = [System.Collections.Generic.List[object]]::new()
foreach ($container in @('mkv','mp4')) {
    $inputName = "av1-opus.$container"
    $inputFile = Join-Path $sourceRoot $inputName
    $sourceProbes[$inputName] = Get-Probe $inputFile
    $sourcePackets[$inputName] = @(Get-PacketHashes $inputFile)
    $sourceVideo = @($sourceProbes[$inputName].streams | Where-Object codec_type -EQ 'video')[0]
    $sourceAudios = @($sourceProbes[$inputName].streams | Where-Object codec_type -EQ 'audio')
    if ($sourceVideo.codec_name -ne 'av1' -or $sourceAudios.Count -ne 2 -or @($sourceAudios | Where-Object codec_name -NE 'opus').Count -ne 0) { throw 'Unexpected source codecs/tracks' }
    foreach ($language in @(@{short='fr';tag='fra';ordinal=0},@{short='en';tag='eng';ordinal=1})) {
        $outputName = "av1-opus-$container-$($language.short).$container"
        $outputFile = Join-Path $outputRoot $outputName
        $arguments = @('-hide_banner','-v','error','-n','-i',$inputFile,'-map','0:v:0','-map',"0:a:$($language.ordinal)",'-c','copy','-metadata:s:a:0',"language=$($language.tag)")
        if ($container -eq 'mp4') { $arguments += @('-movflags','+faststart') }
        $arguments += $outputFile
        $remuxTimer = [System.Diagnostics.Stopwatch]::StartNew()
        Invoke-MediaTool 'ffmpeg' $arguments | Out-Null
        $remuxTimer.Stop()
        $probe = Get-Probe $outputFile
        $packets = @(Get-PacketHashes $outputFile)
        $streams = @($probe.streams)
        if ($streams.Count -ne 2 -or $streams[0].codec_name -ne 'av1' -or $streams[1].codec_name -ne 'opus' -or $streams[1].tags.language -ne $language.tag) { throw "Unexpected output tracks: $outputName" }
        $mapping = @(@{source=[int]$sourceVideo.index;output=[int]$streams[0].index},@{source=[int]$sourceAudios[$language.ordinal].index;output=[int]$streams[1].index})
        $comparisons = @()
        foreach ($map in $mapping) {
            $expected = @($sourcePackets[$inputName] | Where-Object stream_index -EQ $map.source | ForEach-Object data_hash)
            $actual = @($packets | Where-Object stream_index -EQ $map.output | ForEach-Object data_hash)
            if ($expected.Count -eq 0 -or ($expected -join ',') -cne ($actual -join ',')) { throw "Encoded packet mismatch: $outputName stream $($map.output)" }
            $comparisons += @{source_stream_index=$map.source;output_stream_index=$map.output;packet_count=$actual.Count;all_payload_sha256_identical=$true}
        }
        Invoke-MediaTool 'ffmpeg' @('-hide_banner','-v','error','-xerror','-i',$outputFile,'-map','0','-f','null','-') | Out-Null
        $results.Add(@{file=$outputName;source=$inputName;language=$language.short;remux_ms=$remuxTimer.Elapsed.TotalMilliseconds;sha256=(Get-FileHash -Algorithm SHA256 -LiteralPath $outputFile).Hash;bytes=(Get-Item -LiteralPath $outputFile).Length;probe=$probe;packet_hashes=$packets;packet_comparisons=$comparisons;decode_all_tracks='passed'})
    }
}
$after = @{}
foreach ($name in $originalNames) {
    $after[$name] = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $sourceRoot $name)).Hash
    if ($before[$name] -cne $after[$name]) { throw "Original modified: $name" }
}
$totalTimer.Stop()
$ffmpegVersion = Invoke-MediaTool 'ffmpeg' @('-version')
$ffprobeVersion = Invoke-MediaTool 'ffprobe' @('-version')
@{generated_utc=[DateTime]::UtcNow.ToString('o');ffmpeg=$ffmpegVersion[0];ffprobe=$ffprobeVersion[0];commands=$commands;total_prepare_and_verify_ms=$totalTimer.Elapsed.TotalMilliseconds;original_sha256_before=$before;original_sha256_after=$after;source_probes=$sourceProbes;source_packet_hashes=$sourcePackets;outputs=$results;originals_unchanged=$true;packet_payloads_identical=$true;decode_all_tracks='passed'} |
    ConvertTo-Json -Depth 40 | Set-Content -LiteralPath (Join-Path $outputRoot 'generation.json') -Encoding utf8
$results | Select-Object file,remux_ms,bytes | Format-Table
Write-Output 'Four language remuxes decoded; every selected encoded packet identical; four originals unchanged.'

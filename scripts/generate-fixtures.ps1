param([string]$OutputDirectory = 'tests/fixtures/generated')
$ErrorActionPreference = 'Stop'
# Refuse overwriting any existing media, including previously generated fixtures.
if (Test-Path -LiteralPath $OutputDirectory) { throw 'Output directory already exists. Choose a new empty output path.' }
New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
$root = (Resolve-Path -LiteralPath $OutputDirectory).Path
$commands = [System.Collections.Generic.List[object]]::new()
function Invoke-MediaTool([string]$Tool, [string[]]$Arguments) {
    $commands.Add(@{ tool = $Tool; arguments = $Arguments })
    & $Tool @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Tool failed: $LASTEXITCODE" }
}
# testsrc2 and sine are mathematical generators. No third-party media.
# Two Opus tracks deliberately retained in both containers, FR/EN.
Invoke-MediaTool 'ffmpeg' @('-hide_banner','-v','error','-n',
    '-f','lavfi','-i','testsrc2=size=320x180:rate=24',
    '-f','lavfi','-i','sine=frequency=440:sample_rate=48000',
    '-f','lavfi','-i','sine=frequency=880:sample_rate=48000',
    '-map','0:v:0','-map','1:a:0','-map','2:a:0','-t','8',
    '-c:v','libaom-av1','-cpu-used','8','-crf','40','-b:v','0','-g','48','-threads','2',
    '-pix_fmt','yuv420p','-c:a','libopus','-b:a','48k',
    '-metadata:s:a:0','language=fra','-metadata:s:a:0','title=Francais 440 Hz',
    '-metadata:s:a:1','language=eng','-metadata:s:a:1','title=English 880 Hz',
    (Join-Path $root 'av1-opus.mkv'))
Invoke-MediaTool 'ffmpeg' @('-hide_banner','-v','error','-n','-i',(Join-Path $root 'av1-opus.mkv'),'-map','0','-c','copy','-movflags','+faststart',(Join-Path $root 'av1-opus.mp4'))
Invoke-MediaTool 'ffmpeg' @('-hide_banner','-v','error','-n','-i',(Join-Path $root 'av1-opus.mkv'),'-map','0','-c','copy',(Join-Path $root 'av1-opus.webm'))
Invoke-MediaTool 'ffmpeg' @('-hide_banner','-v','error','-n','-i',(Join-Path $root 'av1-opus.mkv'),'-map','0','-c','copy','-movflags','+frag_keyframe+empty_moov+default_base_moof',(Join-Path $root 'av1-opus-fragmented.mp4'))
$checksums = @()
$probes = @{}
$packetHashes = @{}
foreach ($name in @('av1-opus.mkv','av1-opus.mp4','av1-opus.webm','av1-opus-fragmented.mp4')) {
    $file = Join-Path $root $name
    $checksums += @{ file = $name; sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $file).Hash; bytes = (Get-Item -LiteralPath $file).Length }
    $raw = & ffprobe -v error -show_format -show_streams -show_data -of json $file
    if ($LASTEXITCODE -ne 0) { throw 'ffprobe failed' }
    $probe = ($raw -join "`n") | ConvertFrom-Json
    $probe.format.PSObject.Properties.Remove('filename')
    $probes[$name] = $probe
    # Hash encoded packet payloads, separately from container bytes/timestamps.
    $packetsRaw = & ffprobe -v error -show_packets -show_data_hash sha256 -show_entries packet=stream_index,data_hash -of json $file
    if ($LASTEXITCODE -ne 0) { throw 'packet hashing failed' }
    $packetHashes[$name] = (($packetsRaw -join "`n") | ConvertFrom-Json).packets
    Invoke-MediaTool 'ffmpeg' @('-hide_banner','-v','error','-xerror','-i',$file,'-map','0','-f','null','-')
}
$reference = $packetHashes['av1-opus.mkv']
foreach ($name in @('av1-opus.mp4','av1-opus.webm','av1-opus-fragmented.mp4')) {
    foreach ($streamIndex in @(0,1,2)) {
        $before = @($reference | Where-Object stream_index -EQ $streamIndex | ForEach-Object data_hash)
        $after = @($packetHashes[$name] | Where-Object stream_index -EQ $streamIndex | ForEach-Object data_hash)
        if (($before -join ',') -cne ($after -join ',')) { throw "Encoded packet mismatch: $name stream $streamIndex" }
    }
}
@{ generated = (Get-Date -Format 'yyyy-MM-dd'); ffmpeg = (& ffmpeg -version | Select-Object -First 1); ffprobe = (& ffprobe -version | Select-Object -First 1); commands = $commands; checksums = $checksums; probes = $probes; packet_payloads_identical = $true; decode_all_tracks = 'passed' } |
    ConvertTo-Json -Depth 30 | Set-Content -LiteralPath (Join-Path $root 'generation.json') -Encoding utf8
Write-Output 'Four fixtures decoded successfully; encoded packet payloads identical in all remuxes.'

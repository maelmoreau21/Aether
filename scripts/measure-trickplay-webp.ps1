param([string]$Ffmpeg = 'ffmpeg', [string]$OutputDir = 'target/trickplay-webp-bench')
$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
$destination = [IO.Path]::GetFullPath((Join-Path $repository $OutputDir))
New-Item -ItemType Directory -Path $destination -Force | Out-Null
$source = Join-Path $destination 'synthetic-pq.mkv'
# Legal synthetic 10-bit PQ-tagged test pattern; not a calibrated real HDR scene.
& $Ffmpeg -hide_banner -loglevel error -nostdin -y -f lavfi -i 'testsrc2=size=1280x720:rate=1' -t 10 -pix_fmt yuv420p10le -c:v libaom-av1 -cpu-used 8 -crf 38 -aom-params color-primaries=9:transfer-characteristics=16:matrix-coefficients=9 -color_primaries bt2020 -color_trc smpte2084 -colorspace bt2020nc $source
if ($LASTEXITCODE) { throw 'Synthetic PQ fixture generation failed.' }
$measurements = foreach ($scenario in @('jpeg', 'webp', 'webp-tonemap')) {
    $extension = if ($scenario -eq 'jpeg') { 'jpg' } else { 'webp' }
    $output = Join-Path $destination "$scenario.$extension"
    $filter = if ($scenario -eq 'webp-tonemap') {
        'fps=1,zscale=w=320:h=180:t=linear:npl=100,format=gbrpf32le,tonemap=tonemap=hable:desat=0,zscale=p=bt709:t=bt709:m=bt709:r=tv,format=yuv420p,tile=5x2'
    } elseif ($scenario -eq 'jpeg') {
        'fps=1,scale=320:-2:flags=fast_bilinear,format=yuvj420p,tile=5x2'
    } else { 'fps=1,scale=320:-2:flags=fast_bilinear,format=yuv420p,tile=5x2' }
    $codec = if ($scenario -eq 'jpeg') { @('-c:v','mjpeg','-q:v','7') } else { @('-c:v','libwebp','-quality','86','-compression_level','3') }
    $watch = [Diagnostics.Stopwatch]::StartNew()
    $lines = & $Ffmpeg -hide_banner -benchmark -nostdin -y -threads 1 -filter_threads 1 -i $source -an -sn -dn -vf $filter -frames:v 1 @codec -update 1 $output 2>&1
    $watch.Stop()
    $lines | Set-Content -LiteralPath (Join-Path $destination "$scenario.log")
    if ($LASTEXITCODE) { throw "Trickplay scenario $scenario failed." }
    [pscustomobject]@{ scenario=$scenario; bytes=(Get-Item -LiteralPath $output).Length; processWallMs=$watch.Elapsed.TotalMilliseconds; ffmpegBenchmark=@($lines | Where-Object { "$_" -match '^bench:' } | ForEach-Object { "$_" }) }
}
[pscustomobject]@{ scope='One 5x2 sheet from a synthetic PQ-tagged AV1 pattern. Single software decoder/filter thread. Not calibrated HDR or NAS evidence.'; measurements=@($measurements) } | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $destination 'measurement.json') -Encoding utf8
Get-Content -LiteralPath (Join-Path $destination 'measurement.json')

param([string]$TestBinary, [string]$OutputDir = 'target/artwork-webp-bench')
$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent $PSScriptRoot
if (-not $TestBinary) {
    $TestBinary = (Get-ChildItem -LiteralPath (Join-Path $repository 'target/debug/deps') -Filter 'media_node-*.exe' | Sort-Object LastWriteTime -Descending | Select-Object -First 1).FullName
}
if (-not $TestBinary) { throw 'Build first: cargo test -p media-node --lib --no-run' }
$TestBinary = (Resolve-Path -LiteralPath $TestBinary).Path
$outputPath = [IO.Path]::GetFullPath((Join-Path $repository $OutputDir))
New-Item -ItemType Directory -Path $outputPath -Force | Out-Null
$previousMode = $env:MEDIA_ARTWORK_BENCH_MODE
$previousDir = $env:MEDIA_ARTWORK_BENCH_DIR
try {
    $env:MEDIA_ARTWORK_BENCH_DIR = $outputPath
    $measurements = foreach ($mode in @('legacy', 'webp')) {
        $env:MEDIA_ARTWORK_BENCH_MODE = $mode
        $watch = [Diagnostics.Stopwatch]::StartNew()
        $process = Start-Process -FilePath $TestBinary -ArgumentList @('--exact', 'image_encoding::tests::measure_artwork_encoding', '--ignored', '--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $outputPath "$mode.stdout.txt") -RedirectStandardError (Join-Path $outputPath "$mode.stderr.txt")
        $peak = 0L; $cpuMs = 0.0
        while (-not $process.HasExited) {
            $process.Refresh()
            try { $peak = [Math]::Max($peak, $process.PeakWorkingSet64); $cpuMs = [Math]::Max($cpuMs, $process.TotalProcessorTime.TotalMilliseconds) } catch { }
            Start-Sleep -Milliseconds 50
        }
        $process.WaitForExit(); $watch.Stop()
        if ($process.ExitCode -ne 0) { throw "Benchmark $mode failed; inspect its stderr/stdout." }
        [pscustomobject]@{ mode = $mode; processWallMs = $watch.Elapsed.TotalMilliseconds; sampledCpuMs = $cpuMs; peakWorkingSetBytes = $peak; encoding = Get-Content -LiteralPath (Join-Path $outputPath "$mode.json") -Raw | ConvertFrom-Json }
    }
    $report = [pscustomobject]@{ scope = 'Synthetic encoding only, Windows local test executable. CPU and peak RSS sampled every 50 ms. Not a NAS, playback or production benchmark.'; utc = [DateTime]::UtcNow.ToString('o'); binary = (Split-Path -Leaf $TestBinary); measurements = @($measurements) }
    $report | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $outputPath 'measurement.json') -Encoding utf8
    $report | ConvertTo-Json -Depth 8
} finally {
    $env:MEDIA_ARTWORK_BENCH_MODE = $previousMode
    $env:MEDIA_ARTWORK_BENCH_DIR = $previousDir
}

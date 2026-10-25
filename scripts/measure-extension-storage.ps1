# Real SQLx engine timings, separate from Wasm/IPC and build time.
[CmdletBinding()]
param([ValidateRange(2,100000)][int]$Records = 100000, [switch]$Postgres, [string]$WasmPath = '')
$ErrorActionPreference = 'Stop'
$workspacePath = Split-Path -Parent $PSScriptRoot
$evidencePath = Join-Path $workspacePath 'target/extension-storage-bench'
[IO.Directory]::CreateDirectory($evidencePath) | Out-Null
$previousRecords = $env:MEDIA_STORAGE_BENCH_RECORDS
$previousWasm = $env:MEDIA_STORAGE_BENCH_WASM
Push-Location $workspacePath
try {
    $env:MEDIA_STORAGE_BENCH_RECORDS = [string]$Records
    if ($WasmPath) { $env:MEDIA_STORAGE_BENCH_WASM = (Resolve-Path -LiteralPath $WasmPath).Path }
    $buildLines = @(& cargo test --locked -p media-poc --features query-metrics --test extension_data_bench --no-run --message-format=json -j 1)
    if ($LASTEXITCODE -ne 0) { throw 'Benchmark compilation failed' }
    $executablePath = $null
    foreach ($line in $buildLines) {
        if ($line.StartsWith('{')) {
            $item = $line | ConvertFrom-Json
            if ($item.reason -eq 'compiler-artifact' -and $item.target.name -eq 'extension_data_bench' -and $item.executable) { $executablePath = $item.executable }
        }
    }
    if (-not $executablePath) { throw 'Benchmark executable not found in Cargo artifacts' }
    $binarySha256 = (Get-FileHash -LiteralPath $executablePath -Algorithm SHA256).Hash.ToLowerInvariant()
    $sourceHashes = [ordered]@{}
    foreach ($relativePath in @('crates/core/src/extension_data.rs','crates/core/src/extension_objects.rs','crates/core/src/extensions.rs','crates/core/src/extension_runtime.rs','crates/extension-sdk/src/storage.rs')) {
        $sourceHashes[$relativePath] = (Get-FileHash -LiteralPath (Join-Path $workspacePath $relativePath) -Algorithm SHA256).Hash.ToLowerInvariant()
    }
    $tests = @('sqlite_storage_benchmark')
    if ($Postgres) {
        if (-not $env:MEDIA_TEST_POSTGRES_URL) { throw 'Provide a disposable MEDIA_TEST_POSTGRES_URL; no production database' }
        $tests += 'postgres_storage_benchmark'
    }
    foreach ($testName in $tests) {
        $logPath = Join-Path $evidencePath "$testName.log"
        $errorPath = Join-Path $evidencePath "$testName.stderr.log"
        $process = Start-Process -FilePath $executablePath -ArgumentList @('--exact',$testName,'--ignored','--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput $logPath -RedirectStandardError $errorPath
        $sampledPeakRss = 0L
        $sampledCpuMs = 0.0
        while (-not $process.HasExited) {
            $process.Refresh()
            $sampledPeakRss = [Math]::Max($sampledPeakRss,$process.WorkingSet64)
            $sampledCpuMs = [Math]::Max($sampledCpuMs,$process.TotalProcessorTime.TotalMilliseconds)
            Start-Sleep -Milliseconds 100
        }
        $process.WaitForExit()
        Get-Content -LiteralPath $logPath
        if ($process.ExitCode -ne 0) { Get-Content -LiteralPath $errorPath; throw "$testName failed" }
        $metrics = [ordered]@{test=$testName;sampledPeakRssBytes=$sampledPeakRss;sampledCpuMs=$sampledCpuMs;sampleIntervalMs=100;binarySha256=$binarySha256;sourceHashes=$sourceHashes;includes='whole benchmark process incl seeding, runtime and assertions; excludes compilation and PostgreSQL server'}
        $metrics | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidencePath "$testName.process.json") -Encoding utf8
        $metrics | ConvertTo-Json -Compress
    }
}
finally {
    $env:MEDIA_STORAGE_BENCH_RECORDS = $previousRecords
    $env:MEDIA_STORAGE_BENCH_WASM = $previousWasm
    Pop-Location
}

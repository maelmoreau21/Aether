param(
  [string]$Snapshot = 'target/linux-mission-3-2-snapshot',
  [string]$Evidence = 'target/linux-mission-3-2-evidence/final-node-tests.log',
  [switch]$StreamingOnly,
  [switch]$NativeOnly,
  [switch]$AllocationCheck
)
$ErrorActionPreference = 'Stop'
$workspacePath = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$snapshotPath = (Resolve-Path (Join-Path $workspacePath $Snapshot)).Path
if (-not $snapshotPath.StartsWith($workspacePath + [IO.Path]::DirectorySeparatorChar)) { throw 'Snapshot must stay inside workspace' }
$buildPath = Join-Path $workspacePath 'target/linux-mission-3-2-build'
$cachePath = Join-Path $workspacePath 'target/linux-mission-3-2-cargo-online'
$fixturePath = Join-Path $workspacePath 'tests/fixtures/generated'
$ffmpegPath = Join-Path $workspacePath 'target/ffmpeg-linux-9.0.2'
$logPath = Join-Path $workspacePath $Evidence
New-Item -ItemType Directory -Force $buildPath,$cachePath,(Split-Path $logPath) | Out-Null
$testCommand = 'cargo test --locked --offline -p media-node --lib -j 1 -- --test-threads=1'
if ($StreamingOnly) { $testCommand = @('mp4_audio::','video_stream::','files::','stream::') | ForEach-Object { "cargo test --locked --offline -p media-node --lib -j 1 $_ -- --test-threads=1" }; $testCommand = ($testCommand | ForEach-Object { if ($_ -match ' 1 stream:: ') { $_ + ' --skip video_stream::' } else { $_ } }) -join ' && ' }
if ($NativeOnly) { $testCommand = 'cargo test --locked --offline -p media-node --lib -j 1 mp4_audio:: -- --test-threads=1' }
if ($AllocationCheck) { $testCommand = 'cargo check --locked --offline -p media-node --example streaming_allocations -j 1' }
$dockerArguments = @('run','--rm','--read-only','--network','none','--tmpfs','/tmp:rw,exec,size=2g',
  '--mount',"type=bind,source=$snapshotPath,target=/source,readonly",
  '--mount',"type=bind,source=$fixturePath,target=/source/tests/fixtures/generated,readonly",
  '--mount',"type=bind,source=$ffmpegPath,target=/ffmpeg,readonly",
  '--mount',"type=bind,source=$buildPath,target=/build",
  '--mount',"type=bind,source=$cachePath,target=/cache",
  '-e','CARGO_HOME=/cache','-e','RUSTUP_HOME=/usr/local/rustup',
  '-e','PATH=/ffmpeg/bin:/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin',
  '-w','/source','media-ci-stable:20261021','bash','-c',
  $testCommand)
& docker @dockerArguments 2>&1 | Tee-Object -FilePath $logPath
exit $LASTEXITCODE

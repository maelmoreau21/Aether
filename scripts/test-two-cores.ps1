# Disposable two-process PostgreSQL smoke: startup, shared schema, abrupt loss.
$ErrorActionPreference = 'Stop'
$container = $null
$cores = @()
$saved = @{}
foreach ($name in @('POSTGRES_PASSWORD','MEDIA_DB_ENGINE','MEDIA_DB_URL','MEDIA_PORT','MEDIA_PUBLIC_URL','MEDIA_PLUGINS_PATH','MEDIA_ASSETS_PATH','MEDIA_DB_PATH')) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
$work = Join-Path ([System.IO.Path]::GetTempPath()) ('media-two-cores-' + [Guid]::NewGuid().ToString('N'))
try {
    New-Item -ItemType Directory -Path $work | Out-Null
    $env:POSTGRES_PASSWORD = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
    $password = $env:POSTGRES_PASSWORD
    $container = (& docker run --detach --name ('media-two-cores-' + [Guid]::NewGuid().ToString('N')) `
        --tmpfs /var/lib/postgresql --env POSTGRES_PASSWORD --env POSTGRES_DB=media_test `
        --publish '127.0.0.1::5432' postgres:18-alpine).Trim()
    if ($LASTEXITCODE -ne 0 -or $container -notmatch '^[a-f0-9]{64}$') { throw 'Could not start test PostgreSQL.' }
    for ($i = 0; $i -lt 120; $i++) {
        & docker exec $container pg_isready -U postgres -d media_test *> $null
        if ($LASTEXITCODE -eq 0) { break }
        Start-Sleep -Milliseconds 500
    }
    if ($LASTEXITCODE -ne 0) { throw 'Test PostgreSQL did not become ready.' }
    $portLine = (& docker port $container 5432/tcp).Trim()
    if ($portLine -notmatch '^127\.0\.0\.1:([0-9]+)$') { throw 'Could not identify test PostgreSQL port.' }
    $env:MEDIA_DB_ENGINE = 'postgres'
    $env:MEDIA_DB_URL = "postgresql://postgres:${password}@127.0.0.1:$($Matches[1])/media_test?sslmode=disable"
    $env:POSTGRES_PASSWORD = $null
    $binary = Join-Path (Split-Path -Parent $PSScriptRoot) 'target/debug/aether-core.exe'
    & cargo build --locked -p media-poc --bin aether-core -j 1
    if ($LASTEXITCODE -ne 0) { throw 'Could not build Core.' }
    $startup = [System.Diagnostics.Stopwatch]::StartNew()
    for ($i = 1; $i -le 2; $i++) {
        $coreDir = Join-Path $work "core$i"
        New-Item -ItemType Directory -Path $coreDir | Out-Null
        $listener = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
        $listener.Start()
        $port = $listener.LocalEndpoint.Port
        $listener.Stop()
        $env:MEDIA_PORT = "$port"
        $env:MEDIA_PUBLIC_URL = "http://127.0.0.1:$port"
        $env:MEDIA_PLUGINS_PATH = Join-Path $coreDir 'plugins'
        $env:MEDIA_ASSETS_PATH = Join-Path $coreDir 'assets'
        $env:MEDIA_DB_PATH = Join-Path $coreDir 'bootstrap-only.sqlite'
        $process = Start-Process -FilePath $binary -WorkingDirectory $coreDir -WindowStyle Hidden -PassThru `
            -RedirectStandardOutput (Join-Path $coreDir 'stdout.log') -RedirectStandardError (Join-Path $coreDir 'stderr.log')
        $cores += @{ Process = $process; Port = $port }
    }
    foreach ($core in $cores) {
        $ready = $false
        for ($i = 0; $i -lt 120; $i++) {
            if ($core.Process.HasExited) { throw 'A Core exited before becoming ready.' }
            try {
                $reply = Invoke-RestMethod -Uri "http://127.0.0.1:$($core.Port)/api/v1/config" -TimeoutSec 2
                if ($reply) { $ready = $true; break }
            } catch { Start-Sleep -Milliseconds 500 }
        }
        if (-not $ready) { throw 'A Core did not become ready.' }
    }
    $startup.Stop()
    $rss = @($cores | ForEach-Object { [math]::Round((Get-Process -Id $_.Process.Id).WorkingSet64 / 1MB, 1) })
    Write-Host "Two Core processes ready in $($startup.ElapsedMilliseconds) ms; RSS MiB: $($rss -join ', ')."
    $bootstrapFile = Join-Path $work 'core1/.bootstrap_token'
    if (-not (Test-Path -LiteralPath $bootstrapFile)) { throw 'Bootstrap token file missing.' }
    $bootstrapToken = [System.IO.File]::ReadAllText($bootstrapFile).Trim()
    $adminPassword = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
    $origin = "http://127.0.0.1:$($cores[0].Port)"
    $body = @{ login = 'core-smoke-admin'; password = $adminPassword; bootstrap_token = $bootstrapToken } | ConvertTo-Json -Compress
    $session = $null
    $null = Invoke-RestMethod -Method Post -Uri "$origin/api/v1/auth/bootstrap" -Headers @{ Origin = $origin } `
        -ContentType 'application/json' -Body $body -SessionVariable session
    $bootstrapToken = $null
    $adminPassword = $null
    $peerOrigin = "http://127.0.0.1:$($cores[1].Port)"
    $identity = Invoke-RestMethod -Uri "$peerOrigin/api/v1/auth/me" -Headers @{ Origin = $peerOrigin } -WebSession $session
    if ($identity.user.role -ne 'administrator') { throw 'Session created on Core 1 was not accepted by Core 2.' }
    Write-Host 'Session created on Core 1 was accepted by Core 2.'
    $failover = [System.Diagnostics.Stopwatch]::StartNew()
    Stop-Process -Id $cores[0].Process.Id -Force
    $reply = Invoke-RestMethod -Uri "http://127.0.0.1:$($cores[1].Port)/api/v1/config" -TimeoutSec 5
    if (-not $reply) { throw 'Surviving Core stopped serving after peer loss.' }
    $failover.Stop()
    Write-Host "Surviving Core responded after abrupt peer termination in $($failover.ElapsedMilliseconds) ms."
}
finally {
    foreach ($core in $cores) {
        if (-not $core.Process.HasExited) { Stop-Process -Id $core.Process.Id -Force }
    }
    if ($container -match '^[a-f0-9]{64}$') { & docker rm --force $container *> $null }
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
    $resolved = [System.IO.Path]::GetFullPath($work)
    $parent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    if ($resolved.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase) -and
        [System.IO.Path]::GetFileName($resolved) -match '^media-two-cores-[a-f0-9]{32}$') {
        Remove-Item -LiteralPath $resolved -Recurse -Force
    }
}

# Requires Docker, Rust and the generated legal media fixtures used by the tests.
# Runs only a disposable PostgreSQL 18 instance; never targets an existing database.
[CmdletBinding()]
param([switch]$OnlyDistributed, [switch]$OnlySubtitles, [switch]$OnlyNotifications, [switch]$OnlyExtensionRuns, [switch]$OnlyMultiCoreExtension, [string]$TestBinary)

$ErrorActionPreference = 'Stop'
if ($TestBinary) {
    $testBinaryPath = (Resolve-Path -LiteralPath $TestBinary).Path
    $expectedDirectory = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../target/debug/deps'))
    if ([System.IO.Path]::GetDirectoryName($testBinaryPath) -ne $expectedDirectory -or
        [System.IO.Path]::GetFileName($testBinaryPath) -notmatch '^aether_core-[a-f0-9]+\.exe$' -or
        -not ($OnlySubtitles -or $OnlyNotifications -or $OnlyExtensionRuns)) {
        throw 'TestBinary requires a freshly built Core unit-test executable and one matching targeted group.'
    }
}
if ((@($OnlyDistributed, $OnlySubtitles, $OnlyNotifications, $OnlyExtensionRuns, $OnlyMultiCoreExtension) | Where-Object { $_ }).Count -gt 1) {
    throw 'Choose one targeted PostgreSQL test group.'
}
$containerId = $null
$previousPassword = [Environment]::GetEnvironmentVariable('POSTGRES_PASSWORD', 'Process')
$previousTestUrl = [Environment]::GetEnvironmentVariable('MEDIA_TEST_POSTGRES_URL', 'Process')
$previousPluginsPath = [Environment]::GetEnvironmentVariable('MEDIA_PLUGINS_PATH', 'Process')
$previousAssetsPath = [Environment]::GetEnvironmentVariable('MEDIA_ASSETS_PATH', 'Process')
$testStorage = $null
$pushedLocation = $false

function Invoke-CheckedCargo {
    param([Parameter(Mandatory)][string[]]$CargoArguments)
    if ($TestBinary) {
        $libIndex = [Array]::IndexOf($CargoArguments, '--lib')
        if ($libIndex -lt 0 -or $libIndex + 1 -ge $CargoArguments.Length) { throw 'Unit test filter missing.' }
        & $testBinaryPath $CargoArguments[$libIndex + 1] --ignored --test-threads=1
    } else {
        & cargo @CargoArguments
    }
    if ($LASTEXITCODE -ne 0) {
        throw 'PostgreSQL validation failed; the disposable database will be removed.'
    }
}

try {
    Push-Location (Split-Path -Parent $PSScriptRoot)
    $pushedLocation = $true
    $containerName = 'media-pg18-test-' + [Guid]::NewGuid().ToString('N')
    $testPassword = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
    $env:POSTGRES_PASSWORD = $testPassword

    # Only the environment variable name enters argv. PostgreSQL stores its
    # disposable database in RAM; no host bind or persistent volume is created.
    $runOutput = @(& docker run --detach --name $containerName `
        --tmpfs /var/lib/postgresql `
        --env POSTGRES_PASSWORD --env POSTGRES_DB=media_test `
        --publish '127.0.0.1::5432' postgres:18-alpine)
    if ($LASTEXITCODE -ne 0) {
        throw 'Could not start the disposable PostgreSQL 18 container.'
    }
    $returnedId = ($runOutput -join '').Trim()
    if ($returnedId -notmatch '^[a-f0-9]{64}$') {
        throw 'Docker did not return a valid container ID; refusing container operations.'
    }
    $containerId = $returnedId
    [Environment]::SetEnvironmentVariable('POSTGRES_PASSWORD', $previousPassword, 'Process')

    $ready = $false
    for ($attempt = 0; $attempt -lt 120; $attempt++) {
        & docker exec $containerId pg_isready --host 127.0.0.1 --username postgres --dbname media_test *> $null
        if ($LASTEXITCODE -eq 0) {
            $ready = $true
            break
        }
        Start-Sleep -Milliseconds 500
    }
    if (-not $ready) {
        throw 'Disposable PostgreSQL 18 did not become ready within 60 seconds.'
    }

    $portOutput = @(& docker port $containerId 5432/tcp)
    if ($LASTEXITCODE -ne 0 -or ($portOutput -join '').Trim() -notmatch '^127\.0\.0\.1:([0-9]+)$') {
        throw 'Could not identify the loopback PostgreSQL test port.'
    }
    $testPort = $Matches[1]
    $env:MEDIA_TEST_POSTGRES_URL = "postgresql://postgres:${testPassword}@127.0.0.1:${testPort}/media_test?sslmode=disable"
    $testStorage = Join-Path ([System.IO.Path]::GetTempPath()) ('media-pg18-storage-' + [Guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $testStorage | Out-Null
    $env:MEDIA_PLUGINS_PATH = Join-Path $testStorage 'plugins'
    $env:MEDIA_ASSETS_PATH = Join-Path $testStorage 'assets'
    Write-Host 'PostgreSQL 18 is ready on a temporary loopback port. Running serial contract tests.'

    if ($OnlySubtitles) {
        Invoke-CheckedCargo -CargoArguments @('test', '--locked', '-p', 'aether-core', '-j', '1', '--lib', 'postgres_subtitle_schema_reservations_and_derivatives_survive_reopen', '--', '--ignored', '--test-threads=1')
        Write-Host 'Targeted PostgreSQL subtitle tests passed.'
        return
    }
    if ($OnlyExtensionRuns) {
        Invoke-CheckedCargo -CargoArguments @('test', '--locked', '-p', 'aether-core', '-j', '1', '--lib', 'postgres_history_owner_is_fenced_between_independent_pools', '--', '--ignored', '--test-threads=1')
        Write-Host 'Targeted PostgreSQL extension-run ownership tests passed.'
        return
    }
    if ($OnlyMultiCoreExtension) {
        Invoke-CheckedCargo -CargoArguments @('test', '--locked', '-p', 'aether-core', '-j', '1', '--test', 'multi_core_extension_runs', '--', '--ignored', '--nocapture', '--test-threads=1')
        Write-Host 'Targeted two-process Core extension crash test passed.'
        return
    }
    if ($OnlyNotifications) {
        Invoke-CheckedCargo -CargoArguments @('test', '--locked', '-p', 'aether-core', '-j', '1', '--lib', 'postgres_notification_claim_is_fenced_and_queue_survives_reopen', '--', '--ignored', '--test-threads=1')
        Write-Host 'Targeted PostgreSQL notification tests passed.'
        return
    }
    if (-not $OnlyDistributed) {
        Invoke-CheckedCargo -CargoArguments @('test', '--locked', '-p', 'aether-core', '-j', '1', '--test', 'postgres', '--', '--ignored', '--test-threads=1')
        Invoke-CheckedCargo -CargoArguments @('test', '--locked', '-p', 'aether-core', '-j', '1', '--lib', 'oidc::tests', '--', '--test-threads=1')
    }
    Invoke-CheckedCargo -CargoArguments @('test', '--locked', '-p', 'aether-core', '-j', '1', '--test', 'distributed_multi_node', '--', '--test-threads=1')
    Write-Host 'Requested PostgreSQL tests passed.'
}
finally {
    [Environment]::SetEnvironmentVariable('POSTGRES_PASSWORD', $previousPassword, 'Process')
    [Environment]::SetEnvironmentVariable('MEDIA_TEST_POSTGRES_URL', $previousTestUrl, 'Process')
    [Environment]::SetEnvironmentVariable('MEDIA_PLUGINS_PATH', $previousPluginsPath, 'Process')
    [Environment]::SetEnvironmentVariable('MEDIA_ASSETS_PATH', $previousAssetsPath, 'Process')
    $testPassword = $null
    if ($containerId -and $containerId -match '^[a-f0-9]{64}$') {
        # Never enumerate or remove other containers: only our returned exact ID.
        & docker rm --force $containerId *> $null
        if ($LASTEXITCODE -ne 0) {
            Write-Warning "Could not remove the disposable container $containerId."
        }
    }
    if ($pushedLocation) {
        Pop-Location
    }
    if ($testStorage) {
        $resolved = [System.IO.Path]::GetFullPath($testStorage)
        $parent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
        if ($resolved.StartsWith($parent, [StringComparison]::OrdinalIgnoreCase) -and
            [System.IO.Path]::GetFileName($resolved) -match '^media-pg18-storage-[a-f0-9]{32}$') {
            Remove-Item -LiteralPath $resolved -Recurse -Force
        }
    }
}

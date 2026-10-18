# Requires Docker, Rust and the generated legal media fixtures used by the tests.
# Runs only a disposable PostgreSQL 18 instance; never targets an existing database.
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$containerId = $null
$previousPassword = [Environment]::GetEnvironmentVariable('POSTGRES_PASSWORD', 'Process')
$previousTestUrl = [Environment]::GetEnvironmentVariable('MEDIA_TEST_POSTGRES_URL', 'Process')
$pushedLocation = $false

function Invoke-CheckedCargo {
    param([Parameter(Mandatory)][string[]]$CargoArguments)
    & cargo @CargoArguments
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
    Write-Host 'PostgreSQL 18 is ready on a temporary loopback port. Running serial contract tests.'

    Invoke-CheckedCargo -CargoArguments @('test', '--locked', '-p', 'media-poc', '-j', '1', '--test', 'postgres', '--', '--ignored', '--test-threads=1')
    Invoke-CheckedCargo -CargoArguments @('test', '--locked', '-p', 'media-poc', '-j', '1', '--lib', 'oidc::tests', '--', '--test-threads=1')
    Invoke-CheckedCargo -CargoArguments @('test', '--locked', '-p', 'media-poc', '-j', '1', '--test', 'distributed_multi_node', '--', '--test-threads=1')
    Write-Host 'PostgreSQL contract, OIDC and distributed Node tests passed.'
}
finally {
    [Environment]::SetEnvironmentVariable('POSTGRES_PASSWORD', $previousPassword, 'Process')
    [Environment]::SetEnvironmentVariable('MEDIA_TEST_POSTGRES_URL', $previousTestUrl, 'Process')
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
}

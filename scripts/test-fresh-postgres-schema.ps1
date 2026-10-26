# Validate the disposable PostgreSQL schema without compiling the Core.
$ErrorActionPreference = 'Stop'
$containerId = $null
$temporarySql = $null
$previousPassword = $env:POSTGRES_PASSWORD
try {
    $env:POSTGRES_PASSWORD = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
    $name = 'media-schema-test-' + [Guid]::NewGuid().ToString('N')
    $containerId = (& docker run --detach --name $name --tmpfs /var/lib/postgresql `
        --env POSTGRES_PASSWORD --env POSTGRES_DB=media_test postgres:18-alpine).Trim()
    if ($LASTEXITCODE -ne 0 -or $containerId -notmatch '^[a-f0-9]{64}$') {
        throw 'Could not create disposable PostgreSQL container.'
    }
    for ($attempt = 0; $attempt -lt 120; $attempt++) {
        & docker exec $containerId pg_isready --username postgres --dbname media_test *> $null
        if ($LASTEXITCODE -eq 0) { break }
        Start-Sleep -Milliseconds 500
    }
    if ($LASTEXITCODE -ne 0) { throw 'PostgreSQL did not become ready.' }
    $source = [System.IO.File]::ReadAllText((Join-Path $PSScriptRoot '../crates/core/schema.sql'))
    $startMarker = '-- AETHER_SCHEMA_POSTGRES_BEGIN'
    $endMarker = '-- AETHER_SCHEMA_POSTGRES_END'
    $start = $source.IndexOf($startMarker) + $startMarker.Length
    $end = $source.IndexOf($endMarker)
    if ($start -lt $startMarker.Length -or $end -le $start) { throw 'PostgreSQL schema section missing.' }
    $temporarySql = Join-Path ([System.IO.Path]::GetTempPath()) ('media-schema-' + [Guid]::NewGuid().ToString('N') + '.sql')
    [System.IO.File]::WriteAllText($temporarySql, $source.Substring($start, $end - $start))
    & docker cp $temporarySql "${containerId}:/tmp/schema.sql" | Out-Null
    if ($LASTEXITCODE -ne 0) { throw 'Could not copy the schema.' }
    & docker exec $containerId psql --username postgres --dbname media_test --set ON_ERROR_STOP=1 --quiet --file /tmp/schema.sql
    if ($LASTEXITCODE -ne 0) { throw 'Fresh PostgreSQL schema failed.' }
    Write-Host 'Fresh PostgreSQL schema: OK'
}
finally {
    $env:POSTGRES_PASSWORD = $previousPassword
    if ($containerId -match '^[a-f0-9]{64}$') {
        & docker rm --force $containerId *> $null
    }
    if ($temporarySql) { Remove-Item -LiteralPath $temporarySql -Force }
}

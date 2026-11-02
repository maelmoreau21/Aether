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
    $timestampCheck = @'
INSERT INTO login_throttle(throttle_key,window_started,attempts) VALUES('timestamp-test',2147483648,1);
INSERT INTO users(id,role,created_at,disabled_at) VALUES('timestamp-test','user',2147483648,2147483648);
INSERT INTO api_keys(id,name,user_id,token_hash,created_at,expires_at,revoked_at,scope) VALUES('timestamp-key','test','timestamp-test','synthetic',2147483648,9999999999,2147483648,'catalog:read');
DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.columns WHERE table_schema='public' AND (column_name LIKE '%\_at' ESCAPE '\' OR column_name='window_started') AND data_type='integer') THEN RAISE EXCEPTION '32-bit timestamp column remains'; END IF;
  IF NOT EXISTS (SELECT 1 FROM api_keys WHERE id='timestamp-key' AND created_at=2147483648 AND expires_at=9999999999 AND revoked_at=2147483648) THEN RAISE EXCEPTION 'API key timestamp mismatch'; END IF;
END $$;
'@
    $timestampCheck | & docker exec -i $containerId psql --username postgres --dbname media_test --set ON_ERROR_STOP=1 --quiet
    if ($LASTEXITCODE -ne 0) { throw 'PostgreSQL timestamp validation failed.' }
    Write-Host 'Fresh PostgreSQL schema: OK'
}
finally {
    $env:POSTGRES_PASSWORD = $previousPassword
    if ($containerId -match '^[a-f0-9]{64}$') {
        & docker rm --force $containerId *> $null
    }
    if ($temporarySql) { Remove-Item -LiteralPath $temporarySql -Force }
}

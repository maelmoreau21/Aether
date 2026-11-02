[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$testContainerId = $null
$testStage = Join-Path ([IO.Path]::GetTempPath()) ('media-admin-pg-' + [Guid]::NewGuid().ToString('N'))
$savedPath = $env:PATH
$savedPgPassword = $env:POSTGRES_PASSWORD
$savedTestUrl = $env:MEDIA_TEST_POSTGRES_URL
$savedContainer = $env:MEDIA_TEST_PG_CONTAINER
New-Item -ItemType Directory -Path $testStage | Out-Null
try {
    $env:POSTGRES_PASSWORD = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
    $testPassword = $env:POSTGRES_PASSWORD
    $testContainerId = (& docker run -d --name ('media-admin-pg-' + [Guid]::NewGuid().ToString('N')) --tmpfs /var/lib/postgresql -e POSTGRES_PASSWORD -e POSTGRES_DB=media_test -p '127.0.0.1::5432' postgres:18-alpine).Trim()
    if ($LASTEXITCODE -ne 0 -or $testContainerId -notmatch '^[a-f0-9]{64}$') { throw 'Disposable PostgreSQL startup failed.' }
    for ($i = 0; $i -lt 100; $i++) {
        & docker exec $testContainerId pg_isready -h 127.0.0.1 -U postgres -d media_test *> $null
        if ($LASTEXITCODE -eq 0) { break }
        Start-Sleep -Milliseconds 500
    }
    $mapping = (& docker port $testContainerId 5432/tcp).Trim()
    if ($mapping -notmatch '^127\.0\.0\.1:(\d+)$') { throw 'Temporary PostgreSQL port invalid.' }
    $env:MEDIA_TEST_POSTGRES_URL = "postgresql://postgres:${testPassword}@127.0.0.1:$($Matches[1])/media_test?sslmode=disable"
    $env:MEDIA_TEST_PG_CONTAINER = $testContainerId
    # Real PostgreSQL clients run in the disposable container. No secret enters argv.
    $wrapperSource = @'
use std::{process::{Command,Stdio},path::Path};
fn main(){
 let args:Vec<_>=std::env::args_os().collect();
 let tool=Path::new(&args[0]).file_stem().unwrap().to_str().unwrap();
 let mut command=Command::new("docker");
 command.args(["exec","-i"]);
 for name in ["PGDATABASE","PGUSER","PGPASSWORD","PGSSLMODE","PGCONNECT_TIMEOUT"] {if std::env::var_os(name).is_some(){command.args(["--env",name]);}}
 command.args(["--env","PGHOST=127.0.0.1","--env","PGPORT=5432"]);
 command.arg(std::env::var_os("MEDIA_TEST_PG_CONTAINER").unwrap()).arg(tool).args(&args[1..]);
 command.stdin(Stdio::inherit()).stdout(Stdio::inherit()).stderr(Stdio::inherit());
 let status=command.status().unwrap();std::process::exit(status.code().unwrap_or(1));
}
'@
    $wrapperPath = Join-Path $testStage 'pg_wrapper.rs'
    [IO.File]::WriteAllText($wrapperPath, $wrapperSource)
    & rustc --crate-name pg_wrapper $wrapperPath -o (Join-Path $testStage 'pg_dump.exe')
    if ($LASTEXITCODE -ne 0) { throw 'Client bridge build failed.' }
    Copy-Item -LiteralPath (Join-Path $testStage 'pg_dump.exe') -Destination (Join-Path $testStage 'pg_restore.exe')
    $env:PATH = $testStage + [IO.Path]::PathSeparator + $env:PATH
    & cargo test --locked -p media-poc --test admin_postgres -- --ignored --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw 'Full PostgreSQL backup/restore validation failed.' }
    Write-Host 'Real PostgreSQL 18 full unencrypted backup and restore passed.'
} finally {
    $env:PATH = $savedPath
    $env:POSTGRES_PASSWORD = $savedPgPassword
    $env:MEDIA_TEST_POSTGRES_URL = $savedTestUrl
    $env:MEDIA_TEST_PG_CONTAINER = $savedContainer
    if ($testContainerId -match '^[a-f0-9]{64}$') { & docker rm -f $testContainerId *> $null }
    $resolvedStage = [IO.Path]::GetFullPath($testStage)
    $resolvedTemp = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if ($resolvedStage.StartsWith($resolvedTemp,[StringComparison]::OrdinalIgnoreCase) -and (Split-Path $resolvedStage -Leaf) -like 'media-admin-pg-*') { Remove-Item -LiteralPath $resolvedStage -Recurse -Force }
}

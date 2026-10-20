param([string]$Path = '.env')
$ErrorActionPreference = 'Stop'
if (Test-Path -LiteralPath $Path) {
    throw 'Destination already exists. Preserve it and choose a new output path.'
}
$filmSecret = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).ToLowerInvariant()
do {
    $seriesSecret = [Convert]::ToHexString([Security.Cryptography.RandomNumberGenerator]::GetBytes(32)).ToLowerInvariant()
} while ($seriesSecret -eq $filmSecret)
$template = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '../.env.example')
$template = $template -replace '(?m)^MEDIA_NODE_FILMS_PAIRING_TOKEN=.*$', "MEDIA_NODE_FILMS_PAIRING_TOKEN=$filmSecret"
$template = $template -replace '(?m)^MEDIA_NODE_SERIES_PAIRING_TOKEN=.*$', "MEDIA_NODE_SERIES_PAIRING_TOKEN=$seriesSecret"
[IO.File]::WriteAllText([IO.Path]::GetFullPath($Path), $template)
Write-Output 'Configuration generated. Keep this file private; pairing secrets were not printed.'

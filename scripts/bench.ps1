<#
.SYNOPSIS
  Suite rápida de benchmark do Nous (tamanho do binário release, testes dos crates de
  infraestrutura e medição multi-URL via medir-ram.ps1).

.EXAMPLE
  .\scripts\bench.ps1
  .\scripts\bench.ps1 -Build -Urls https://example.com,https://doc.rust-lang.org/book/
  .\scripts\bench.ps1 -Offline
#>
param(
  [string[]]$Urls = @(
    "https://example.com/",
    "https://pt.wikipedia.org/wiki/Rust_(linguagem_de_programa%C3%A7%C3%A3o)",
    "https://doc.rust-lang.org/book/"
  ),
  [int]$Espera = 8,
  [int]$IdleSeconds = 5,
  [switch]$Build,
  [switch]$Chrome,
  [switch]$Offline,
  [switch]$RunTests
)

$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
$cargoScript = Join-Path $PSScriptRoot "cargo.ps1"
$ramScript = Join-Path $PSScriptRoot "medir-ram.ps1"
$nousExe = Join-Path $root "target\release\nous.exe"

if ($Build) {
  Write-Host "==> Compilando release..."
  & $cargoScript build --release
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

if ($RunTests) {
  Write-Host "==> Rodando testes de nous-dom, nous-html e nous-net..."
  & $cargoScript test -p nous-dom -p nous-html -p nous-net
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

$binSize = if (Test-Path $nousExe) {
  "{0:N1} MB" -f ((Get-Item $nousExe).Length / 1MB)
} else {
  "não compilado"
}
Write-Host "Binário release ($nousExe): $binSize"

if (-not (Test-Path $nousExe)) {
  Write-Warning "Binário release não encontrado. Execute com -Build ou .\scripts\cargo.ps1 build --release."
  exit 0
}

$targetUrls = $Urls
$tmpFixture = $null
if ($Offline) {
  $tmpFixture = Join-Path $env:TEMP "nous-bench-fixture-$([guid]::NewGuid().ToString('N').Substring(0,8)).html"
  @"
<!doctype html>
<html><head><title>Nous Bench</title><style>body{font-family:sans-serif;margin:24px}</style></head>
<body><h1>Nous Benchmark Offline</h1><p>Fixture local sem acesso de rede.</p></body></html>
"@ | Set-Content -Path $tmpFixture -Encoding UTF8
  $fileUri = ([System.Uri]$tmpFixture).AbsoluteUri
  $targetUrls = @($fileUri)
}

try {
  & $ramScript -Urls $targetUrls -Espera $Espera -IdleSeconds $IdleSeconds -Chrome:$Chrome -Nous $nousExe
} finally {
  if ($tmpFixture -and (Test-Path $tmpFixture)) {
    Remove-Item $tmpFixture -ErrorAction SilentlyContinue
  }
}

<#
.SYNOPSIS
  Mede a RAM (working set somado de todos os processos) do Nous e, opcionalmente, do Chrome
  abrindo as mesmas páginas. Resultado vai para a tela; copie para docs/benchmarks.md.

.EXAMPLE
  .\scripts\medir-ram.ps1 -Urls https://example.com,https://pt.wikipedia.org/wiki/Rust -Espera 10
  .\scripts\medir-ram.ps1 -Urls https://example.com -Chrome
#>
param(
  [string[]]$Urls = @("https://example.com"),
  [int]$Espera = 8,
  [switch]$Chrome,
  [string]$Nous = "$PSScriptRoot\..\target\release\nous.exe"
)

function Measure-Tree([string]$Name) {
  $procs = Get-Process -Name $Name -ErrorAction SilentlyContinue
  if (-not $procs) { return $null }
  [pscustomobject]@{
    Processos = $procs.Count
    RAM_MB    = [math]::Round(($procs | Measure-Object WorkingSet64 -Sum).Sum / 1MB, 1)
    Privada_MB = [math]::Round(($procs | Measure-Object PrivateMemorySize64 -Sum).Sum / 1MB, 1)
  }
}

if (-not (Test-Path $Nous)) { throw "Compile antes: cargo build --release (procurei $Nous)" }

Write-Host "Nous: abrindo $($Urls.Count) página(s)..."
$p = Start-Process -FilePath $Nous -ArgumentList $Urls[0] -PassThru
Start-Sleep -Seconds $Espera
$nous = Measure-Tree "nous"
Stop-Process -Id $p.Id -ErrorAction SilentlyContinue
Write-Host ("Nous   -> {0} processo(s), {1} MB (privada {2} MB)" -f $nous.Processos, $nous.RAM_MB, $nous.Privada_MB)

if ($Chrome) {
  $chromeExe = @(
    "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
    "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe",
    "$env:LOCALAPPDATA\Google\Chrome\Application\chrome.exe"
  ) | Where-Object { Test-Path $_ } | Select-Object -First 1
  if (-not $chromeExe) { Write-Warning "Chrome não encontrado"; return }
  if (Get-Process chrome -ErrorAction SilentlyContinue) {
    Write-Warning "Feche o Chrome antes de medir (a medição soma todos os processos chrome)."; return
  }
  $perfil = Join-Path $env:TEMP "nous-bench-chrome"
  $chromeArgs = @("--user-data-dir=$perfil", "--no-first-run", "--new-window") + $Urls
  Start-Process -FilePath $chromeExe -ArgumentList $chromeArgs | Out-Null
  Start-Sleep -Seconds $Espera
  $ch = Measure-Tree "chrome"
  Get-Process chrome -ErrorAction SilentlyContinue | Stop-Process -ErrorAction SilentlyContinue
  Write-Host ("Chrome -> {0} processo(s), {1} MB (privada {2} MB)" -f $ch.Processos, $ch.RAM_MB, $ch.Privada_MB)
}

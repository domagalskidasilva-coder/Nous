<#
.SYNOPSIS
  Mede a RAM (working set total, working set privado e memória privada) e o tempo de CPU
  (total e delta em repouso por 5s) do Nous e, opcionalmente, do Chrome abrindo as mesmas URLs.
  Imprime um resumo e uma linha Markdown pronta para colar em docs/benchmarks.md.

.EXAMPLE
  .\scripts\medir-ram.ps1 -Urls https://example.com,https://pt.wikipedia.org/wiki/Rust -Espera 10
  .\scripts\medir-ram.ps1 -Urls https://example.com -Chrome
  .\scripts\medir-ram.ps1 -Urls https://example.com,https://doc.rust-lang.org/book/ -IdleSeconds 5 -Cenario "2 abas"
#>
param(
  [string[]]$Urls = @("https://example.com"),
  [int]$Espera = 8,
  [int]$IdleSeconds = 5,
  [switch]$Chrome,
  [switch]$MultiInstance,
  [string]$Cenario = "",
  [string]$Nous = "$PSScriptRoot\..\target\release\nous.exe"
)

$ErrorActionPreference = "Stop"

function Get-TreeSnapshot([string]$ProcessName) {
  $procs = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)
  if ($procs.Count -eq 0) { return $null }

  $pids = @($procs | ForEach-Object { $_.Id })
  $wsBytes = ($procs | Measure-Object WorkingSet64 -Sum).Sum
  $privBytes = ($procs | Measure-Object PrivateMemorySize64 -Sum).Sum

  $cpuMs = 0.0
  foreach ($proc in $procs) {
    try {
      $cpuMs += $proc.TotalProcessorTime.TotalMilliseconds
    } catch {
      # Processo pode ter encerrado durante a leitura
    }
  }

  $privWsBytes = $null
  try {
    $cimProcs = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -ErrorAction Stop |
      Where-Object { $pids -contains [int]$_.IDProcess })
    if ($cimProcs.Count -gt 0) {
      $privWsBytes = ($cimProcs | Measure-Object WorkingSetPrivate -Sum).Sum
    }
  } catch {
    $privWsBytes = $null
  }

  if ($null -eq $privWsBytes) {
    $privWsBytes = $privBytes
  }

  [pscustomobject]@{
    Processos    = $procs.Count
    RAM_MB       = [math]::Round($wsBytes / 1MB, 1)
    PrivWS_MB    = [math]::Round($privWsBytes / 1MB, 1)
    Privada_MB   = [math]::Round($privBytes / 1MB, 1)
    CpuTotal_ms  = $cpuMs
  }
}

function Measure-BrowserSession {
  param(
    [string]$Executable,
    [string[]]$Arguments,
    [string]$ProcessName,
    [int]$SettleSeconds,
    [int]$IdleWaitSeconds,
    [switch]$LaunchPerUrl,
    [string[]]$UrlList
  )

  $started = @()
  if ($LaunchPerUrl -and $UrlList.Count -gt 1) {
    foreach ($u in $UrlList) {
      $started += Start-Process -FilePath $Executable -ArgumentList $u -PassThru
    }
  } else {
    $started += Start-Process -FilePath $Executable -ArgumentList $Arguments -PassThru
  }

  try {
    Start-Sleep -Seconds $SettleSeconds
    $snap1 = Get-TreeSnapshot -ProcessName $ProcessName
    if ($null -eq $snap1) {
      throw "Nenhum processo '$ProcessName' em execução após ${SettleSeconds}s."
    }

    if ($IdleWaitSeconds -gt 0) {
      Start-Sleep -Seconds $IdleWaitSeconds
    }
    $snap2 = Get-TreeSnapshot -ProcessName $ProcessName
    if ($null -eq $snap2) {
      $snap2 = $snap1
    }

    $cpuIdleDelta = [math]::Max(0.0, [math]::Round($snap2.CpuTotal_ms - $snap1.CpuTotal_ms, 1))
    [pscustomobject]@{
      Processos       = $snap2.Processos
      RAM_MB          = $snap2.RAM_MB
      PrivWS_MB       = $snap2.PrivWS_MB
      Privada_MB      = $snap2.Privada_MB
      CpuTotal_ms     = [math]::Round($snap2.CpuTotal_ms, 1)
      CpuIdleDelta_ms = $cpuIdleDelta
    }
  } finally {
    Get-Process -Name $ProcessName -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    foreach ($proc in $started) {
      if ($proc -and -not $proc.HasExited) {
        Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
      }
    }
  }
}

if ($Urls.Count -eq 0) {
  throw "Informe pelo menos uma URL em -Urls."
}
if (-not (Test-Path $Nous)) {
  throw "Compile antes: .\scripts\cargo.ps1 build --release (procurei $Nous)"
}

$nousProcName = [System.IO.Path]::GetFileNameWithoutExtension($Nous)
if (Get-Process -Name $nousProcName -ErrorAction SilentlyContinue) {
  throw "Já existe um processo '$nousProcName' aberto; feche-o antes de medir."
}

$scenarioLabel = if (-not [string]::IsNullOrWhiteSpace($Cenario)) {
  $Cenario
} elseif ($Urls.Count -eq 1) {
  "1 aba ($($Urls[0]))"
} else {
  "$($Urls.Count) abas ($($Urls[0]) ...)"
}

Write-Host "Nous: abrindo $($Urls.Count) URL(s) (espera=${Espera}s, idle=${IdleSeconds}s)..."
$nous = Measure-BrowserSession `
  -Executable $Nous `
  -Arguments $Urls `
  -ProcessName $nousProcName `
  -SettleSeconds $Espera `
  -IdleWaitSeconds $IdleSeconds `
  -LaunchPerUrl:$MultiInstance `
  -UrlList $Urls

Write-Host ("Nous   -> {0} proc(s) | PrivWS {1} MB | WS {2} MB | PrivCommit {3} MB | CPU total {4} ms | CPU idle ({5}s) {6} ms" -f `
  $nous.Processos, $nous.PrivWS_MB, $nous.RAM_MB, $nous.Privada_MB, $nous.CpuTotal_ms, $IdleSeconds, $nous.CpuIdleDelta_ms)

$ch = $null
if ($Chrome) {
  $chromeExe = @(
    "$env:ProgramFiles\Google\Chrome\Application\chrome.exe",
    "${env:ProgramFiles(x86)}\Google\Chrome\Application\chrome.exe",
    "$env:LOCALAPPDATA\Google\Chrome\Application\chrome.exe"
  ) | Where-Object { Test-Path $_ } | Select-Object -First 1

  if (-not $chromeExe) {
    Write-Warning "Chrome não encontrado"
  } elseif (Get-Process chrome -ErrorAction SilentlyContinue) {
    Write-Warning "Feche o Chrome antes de medir (a medição soma todos os processos chrome)."
  } else {
    $perfil = Join-Path $env:TEMP "nous-bench-chrome"
    if (Test-Path $perfil) {
      Remove-Item -Recurse -Force $perfil -ErrorAction SilentlyContinue
    }
    $chromeArgs = @("--user-data-dir=$perfil", "--no-first-run", "--new-window") + $Urls
    Write-Host "Chrome: abrindo $($Urls.Count) URL(s)..."
    try {
      $ch = Measure-BrowserSession `
        -Executable $chromeExe `
        -Arguments $chromeArgs `
        -ProcessName "chrome" `
        -SettleSeconds $Espera `
        -IdleWaitSeconds $IdleSeconds `
        -UrlList $Urls
    } finally {
      if (Test-Path $perfil) {
        Remove-Item -Recurse -Force $perfil -ErrorAction SilentlyContinue
      }
    }

    Write-Host ("Chrome -> {0} proc(s) | PrivWS {1} MB | WS {2} MB | PrivCommit {3} MB | CPU total {4} ms | CPU idle ({5}s) {6} ms" -f `
      $ch.Processos, $ch.PrivWS_MB, $ch.RAM_MB, $ch.Privada_MB, $ch.CpuTotal_ms, $IdleSeconds, $ch.CpuIdleDelta_ms)
  }
}

$dateStr = Get-Date -Format "yyyy-MM-dd"
$version = "0.1.0"
foreach ($tomlPath in @(
  (Join-Path $PSScriptRoot "..\Cargo.toml"),
  (Join-Path $PSScriptRoot "..\crates\nous-app\Cargo.toml")
)) {
  if (Test-Path $tomlPath) {
    $verMatch = Select-String -Path $tomlPath -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
    if ($verMatch) {
      $version = $verMatch.Matches[0].Groups[1].Value
      break
    }
  }
}

$nousCell = "$($nous.PrivWS_MB) MB ($($nous.Processos) proc)"
$chromeCell = if ($null -ne $ch) { "$($ch.PrivWS_MB) MB ($($ch.Processos) proc)" } else { "—" }
$obsCell = "WS $($nous.RAM_MB) MB; CPU total $($nous.CpuTotal_ms) ms; idle ${IdleSeconds}s: $($nous.CpuIdleDelta_ms) ms"
if ($null -ne $ch) {
  $obsCell += " | Chrome CPU total $($ch.CpuTotal_ms) ms, idle ${IdleSeconds}s: $($ch.CpuIdleDelta_ms) ms"
}

Write-Host ""
Write-Host "Markdown (docs/benchmarks.md):"
Write-Host "| Data | Versão do Nous | Cenário | Nous (PrivWS) | Chrome (PrivWS) | Observações |"
Write-Host "|---|---|---|---|---|---|"
Write-Host "| $dateStr | $version | $scenarioLabel | $nousCell | $chromeCell | $obsCell |"

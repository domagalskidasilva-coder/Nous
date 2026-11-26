<#
.SYNOPSIS
  Mede consumo de RAM (PrivWS, WS, Commit privado), processos e CPU ocioso do Nous Gecko.

.EXAMPLE
  .\scripts\medir-gecko.ps1
  .\scripts\medir-gecko.ps1 -Url https://example.com -Espera 8 -IdleSeconds 5
#>
param(
    [string] $Url = '',
    [int] $Espera = 8,
    [int] $IdleSeconds = 5,
    [string] $FirefoxPath = ''
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$runtime = Join-Path $workspace 'target\nous-gecko-runtime'
$launcher = Join-Path $PSScriptRoot 'nous-gecko.ps1'

if (Get-Process -Name 'firefox' -ErrorAction SilentlyContinue) {
    throw 'Feche instâncias abertas do Firefox/Nous Gecko antes de medir para evitar soma de processos.'
}

$benchProfile = Join-Path $runtime 'bench-gecko-profile'
if (Test-Path -LiteralPath $benchProfile) {
    Remove-Item -LiteralPath $benchProfile -Recurse -Force
}

function Get-GeckoMetrics {
    $procs = @(Get-CimInstance Win32_Process -Filter "Name = 'firefox.exe'")
    if ($procs.Count -eq 0) { return $null }
    $pids = @($procs | ForEach-Object { [int]$_.ProcessId })
    $perf = @(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process | Where-Object { $pids -contains [int]$_.IDProcess })
    $privWsMap = @{}
    foreach ($p in $perf) { $privWsMap[[int]$p.IDProcess] = [double]$p.WorkingSetPrivate }

    $rows = @()
    $totalPrivWs = 0.0
    $totalWs = 0.0
    $totalCpuMs = 0.0

    foreach ($pr in $procs) {
        $id = [int]$pr.ProcessId
        $pws = if ($privWsMap.ContainsKey($id)) { $privWsMap[$id] } else { [double]$pr.WorkingSetSize }
        $ws = [double]$pr.WorkingSetSize
        $totalPrivWs += $pws
        $totalWs += $ws

        try {
            $gp = Get-Process -Id $id -ErrorAction Stop
            $totalCpuMs += $gp.TotalProcessorTime.TotalMilliseconds
        } catch {}

        $kind = 'parent'
        if ($pr.CommandLine -match '-contentproc') {
            if ($pr.CommandLine -match '\s(tab|gpu|socket|rdd|utility|gmplugin)\s*$') {
                $kind = $Matches[1]
            } else {
                $kind = 'child'
            }
        }

        $rows += [pscustomobject]@{
            PID       = $id
            Tipo      = $kind
            PrivWS_MB = [math]::Round($pws / 1MB, 1)
            WS_MB     = [math]::Round($ws / 1MB, 1)
        }
    }

    [pscustomobject]@{
        Processos = $procs.Count
        PrivWS_MB = [math]::Round($totalPrivWs / 1MB, 1)
        WS_MB     = [math]::Round($totalWs / 1MB, 1)
        CpuMs     = [math]::Round($totalCpuMs, 1)
        Detalhes  = $rows
    }
}

$label = if ($Url) { $Url } else { 'nous://nova-aba (0 processo de conteúdo)' }
Write-Host "Iniciando Nous Gecko em '$label' (espera=${Espera}s, idle=${IdleSeconds}s)..."

$launchArgs = @{ ProfilePath = $benchProfile; Url = $Url }
if ($FirefoxPath) { $launchArgs['FirefoxPath'] = $FirefoxPath }

$proc = & $launcher @launchArgs

try {
    Start-Sleep -Seconds $Espera
    $m1 = Get-GeckoMetrics
    if (-not $m1) {
        throw "Nenhum processo firefox.exe encontrado após ${Espera}s."
    }

    if ($IdleSeconds -gt 0) {
        Start-Sleep -Seconds $IdleSeconds
    }
    $m2 = Get-GeckoMetrics
    if (-not $m2) { $m2 = $m1 }

    $cpuIdle = [math]::Max(0.0, [math]::Round($m2.CpuMs - $m1.CpuMs, 1))
    Write-Host ''
    $m2.Detalhes | Format-Table -AutoSize
    Write-Host ("RESUMO -> {0} proc(s) | PrivWS {1} MB | WS {2} MB | CPU total {3} ms | CPU idle ({4}s): {5} ms" -f `
        $m2.Processos, $m2.PrivWS_MB, $m2.WS_MB, $m2.CpuMs, $IdleSeconds, $cpuIdle)
} finally {
    Get-Process -Name 'firefox' -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 400
    if (Test-Path -LiteralPath $benchProfile) {
        Remove-Item -LiteralPath $benchProfile -Recurse -Force -ErrorAction SilentlyContinue
    }
}

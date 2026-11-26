<#
.SYNOPSIS
  Compila o executável nativo nous.exe (com ícone Kanso), empacota o runtime Gecko 153 ESR enxuto
  e instala o Nous localmente em %LOCALAPPDATA%\Programs\Nous com atalhos no Menu Iniciar e Área de Trabalho.

.EXAMPLE
  .\scripts\instalar-nous.ps1
  .\scripts\instalar-nous.ps1 -NoLaunch
#>
param(
    [string] $InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Nous'),
    [string] $RuntimeSource = '',
    [switch] $SkipRegisterBrowser,
    [switch] $NoLaunch
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$icoPath = Join-Path $workspace 'assets\nous.ico'

if (-not $RuntimeSource) {
    $candidates = @(
        (Join-Path $workspace 'target\nous-gecko-runtime\core'),
        "$env:ProgramFiles\Mozilla Firefox",
        "${env:ProgramFiles(x86)}\Mozilla Firefox"
    )
    $RuntimeSource = $candidates | Where-Object { Test-Path (Join-Path $_ 'firefox.exe') } | Select-Object -First 1
}
if (-not $RuntimeSource -or -not (Test-Path (Join-Path $RuntimeSource 'firefox.exe'))) {
    throw 'Runtime Firefox 153 ESR não encontrado. Informe -RuntimeSource com a pasta que contém firefox.exe.'
}

# ---------------------------------------------------------------------------
# 1. Generate Kanso multi-size .ico icon if missing
# ---------------------------------------------------------------------------
function New-KansoIcon([string]$OutPath) {
    Add-Type -AssemblyName System.Drawing
    New-Item -ItemType Directory -Path (Split-Path $OutPath -Parent) -Force | Out-Null
    $sizes = @(16, 32, 48, 256)
    $pngBlobs = @()

    foreach ($sz in $sizes) {
        $bmp = New-Object System.Drawing.Bitmap($sz, $sz, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
        $g.Clear([System.Drawing.Color]::Transparent)

        $pad = [float]([math]::Max(1, [math]::Round($sz * 0.06)))
        $rectW = [float]($sz - $pad * 2)
        $rad = [float]([math]::Max(2, [math]::Round($sz * 0.22)))
        $path = New-Object System.Drawing.Drawing2D.GraphicsPath
        $d = $rad * 2
        $path.AddArc($pad, $pad, $d, $d, 180, 90)
        $path.AddArc($pad + $rectW - $d, $pad, $d, $d, 270, 90)
        $path.AddArc($pad + $rectW - $d, $pad + $rectW - $d, $d, $d, 0, 90)
        $path.AddArc($pad, $pad + $rectW - $d, $d, $d, 90, 90)
        $path.CloseFigure()

        $bgBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 20, 20, 19))
        $g.FillPath($bgBrush, $path)
        $borderPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(55, 232, 229, 222), [float]([math]::Max(1, $sz * 0.025)))
        $g.DrawPath($borderPen, $path)

        # Crisp geometric Kanso 'n' + sage dot
        $stroke = [float]([math]::Max(1.4, $sz * 0.085))
        $inkPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(255, 232, 229, 222), $stroke)
        $inkPen.StartCap = [System.Drawing.Drawing2D.LineCap]::Round
        $inkPen.EndCap = [System.Drawing.Drawing2D.LineCap]::Round
        $inkPen.LineJoin = [System.Drawing.Drawing2D.LineJoin]::Round

        $x1 = [float]($sz * 0.30)
        $x2 = [float]($sz * 0.66)
        $yTop = [float]($sz * 0.31)
        $yBot = [float]($sz * 0.69)
        $g.DrawLine($inkPen, $x1, $yBot, $x1, $yTop)
        $g.DrawLine($inkPen, $x1, $yTop, $x2, $yBot)
        $g.DrawLine($inkPen, $x2, $yBot, $x2, $yTop)

        # Serene sage dot
        $dotR = [float]([math]::Max(1.4, $sz * 0.075))
        $sageBrush = New-Object System.Drawing.SolidBrush([System.Drawing.Color]::FromArgb(255, 141, 168, 142))
        $g.FillEllipse($sageBrush, [float]($sz * 0.72 - $dotR), [float]($sz * 0.71 - $dotR), $dotR * 2, $dotR * 2)

        $ms = New-Object System.IO.MemoryStream
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $pngBlobs += ,($ms.ToArray())

        $ms.Dispose(); $sageBrush.Dispose(); $inkPen.Dispose(); $borderPen.Dispose(); $bgBrush.Dispose(); $path.Dispose(); $g.Dispose(); $bmp.Dispose()
    }

    $fs = [System.IO.File]::Create($OutPath)
    $bw = New-Object System.IO.BinaryWriter($fs)
    $bw.Write([uint16]0)
    $bw.Write([uint16]1)
    $bw.Write([uint16]$sizes.Count)

    $offset = 6 + ($sizes.Count * 16)
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $sz = $sizes[$i]
        $blob = $pngBlobs[$i]
        $bw.Write([byte]($(if ($sz -ge 256) { 0 } else { $sz })))
        $bw.Write([byte]($(if ($sz -ge 256) { 0 } else { $sz })))
        $bw.Write([byte]0)
        $bw.Write([byte]0)
        $bw.Write([uint16]1)
        $bw.Write([uint16]32)
        $bw.Write([uint32]$blob.Length)
        $bw.Write([uint32]$offset)
        $offset += $blob.Length
    }
    foreach ($blob in $pngBlobs) { $bw.Write($blob) }
    $bw.Flush(); $bw.Dispose(); $fs.Dispose()
}

if (-not (Test-Path -LiteralPath $icoPath)) {
    Write-Host "Gerando ícone Kanso em $icoPath..."
    New-KansoIcon $icoPath
}

# ---------------------------------------------------------------------------
# 2. Build native nous.exe launcher via cargo.ps1
# ---------------------------------------------------------------------------
Write-Host 'Compilando executável nativo nous.exe (release)...'
& (Join-Path $PSScriptRoot 'cargo.ps1') build --release -p nous-launcher
$builtExe = Join-Path $workspace 'target\release\nous-desktop.exe'
if (-not (Test-Path -LiteralPath $builtExe)) {
    throw "Executável não encontrado após build: $builtExe"
}

# ---------------------------------------------------------------------------
# 3. Stop running installed instances & stage clean directory
# ---------------------------------------------------------------------------
Get-Process -Name 'firefox' -ErrorAction SilentlyContinue | Where-Object {
    $_.Path -and $_.Path.StartsWith($InstallDir, [System.StringComparison]::OrdinalIgnoreCase)
} | Stop-Process -Force -ErrorAction SilentlyContinue
Start-Sleep -Milliseconds 300

New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
$destGecko = Join-Path $InstallDir 'gecko'
$destRuntime = Join-Path $InstallDir 'runtime'
$destExe = Join-Path $InstallDir 'nous.exe'
$destIco = Join-Path $InstallDir 'nous.ico'

Copy-Item -LiteralPath $builtExe -Destination $destExe -Force
Copy-Item -LiteralPath $icoPath -Destination $destIco -Force

if (Test-Path -LiteralPath $destGecko) {
    Remove-Item -LiteralPath $destGecko -Recurse -Force
}
Copy-Item -Path (Join-Path $workspace 'gecko') -Destination $destGecko -Recurse -Force

# Copy trimmed Gecko 153 ESR runtime (skips ~80 MB of unused Firefox browser/ONNX/crashreporter bloat)
Write-Host "Empacotando runtime Gecko enxuto em $destRuntime..."
New-Item -ItemType Directory -Path $destRuntime -Force | Out-Null

$excludeFiles = @(
    'onnxruntime.dll',
    'mozinference.dll',
    'crashreporter.exe',
    'crashhelper.exe',
    'updater.exe',
    'updater.ini',
    'update-settings.ini',
    'pingsender.exe',
    'maintenanceservice.exe',
    'maintenanceservice_installer.exe',
    'default-browser-agent.exe',
    'private_browsing.exe',
    'private_browsing.VisualElementsManifest.xml',
    'firefox.VisualElementsManifest.xml',
    'nmhproxy.exe'
)
$excludeDirs = @('browser', 'uninstall', 'desktop-launcher')

$robocopyArgs = @(
    $RuntimeSource, $destRuntime, '/MIR', '/NFL', '/NDL', '/NJH', '/NJS', '/NC', '/NS',
    '/XF'
) + $excludeFiles + @('/XD') + $excludeDirs
& robocopy @robocopyArgs | Out-Null
if ($LASTEXITCODE -ge 8) {
    throw "Robocopy falhou com código $LASTEXITCODE"
}

$totalMb = [math]::Round(((Get-ChildItem -Path $InstallDir -Recurse -File | Measure-Object Length -Sum).Sum / 1MB), 1)
Write-Host "Instalado em $InstallDir ($totalMb MB total em disco)."

# ---------------------------------------------------------------------------
# 4. Create Start Menu & Desktop Shortcuts
# ---------------------------------------------------------------------------
$wsh = New-Object -ComObject WScript.Shell
$shortcuts = @(
    (Join-Path $env:APPDATA 'Microsoft\Windows\Start Menu\Programs\Nous.lnk'),
    (Join-Path ([Environment]::GetFolderPath('Desktop')) 'Nous.lnk')
)
foreach ($lnkPath in $shortcuts) {
    $sc = $wsh.CreateShortcut($lnkPath)
    $sc.TargetPath = $destExe
    $sc.WorkingDirectory = $InstallDir
    $sc.IconLocation = "$destIco,0"
    $sc.Description = 'Nous — Navegador minimalista Kanso sobre Gecko'
    $sc.Save()
    Write-Host "Atalho criado: $lnkPath"
}

# ---------------------------------------------------------------------------
# 5. Register User-Level Windows Browser Capabilities (no Admin/UAC needed)
# ---------------------------------------------------------------------------
if (-not $SkipRegisterBrowser) {
    $base = 'HKCU:\Software\Clients\StartMenuInternet\Nous'
    New-Item -Path "$base\Capabilities\URLAssociations" -Force | Out-Null
    New-Item -Path "$base\Capabilities\FileAssociations" -Force | Out-Null
    New-Item -Path "$base\DefaultIcon" -Force | Out-Null
    New-Item -Path "$base\shell\open\command" -Force | Out-Null
    Set-ItemProperty -Path $base -Name '(default)' -Value 'Nous'
    Set-ItemProperty -Path "$base\Capabilities" -Name 'ApplicationName' -Value 'Nous'
    Set-ItemProperty -Path "$base\Capabilities" -Name 'ApplicationDescription' -Value 'Navegador minimalista ultra-leve sobre motor Gecko.'
    Set-ItemProperty -Path "$base\Capabilities" -Name 'ApplicationIcon' -Value "$destIco,0"
    Set-ItemProperty -Path "$base\Capabilities\URLAssociations" -Name 'http' -Value 'NousHTML'
    Set-ItemProperty -Path "$base\Capabilities\URLAssociations" -Name 'https' -Value 'NousHTML'
    Set-ItemProperty -Path "$base\Capabilities\FileAssociations" -Name '.htm' -Value 'NousHTML'
    Set-ItemProperty -Path "$base\Capabilities\FileAssociations" -Name '.html' -Value 'NousHTML'
    Set-ItemProperty -Path "$base\Capabilities\FileAssociations" -Name '.pdf' -Value 'NousHTML'
    Set-ItemProperty -Path "$base\DefaultIcon" -Name '(default)' -Value "$destIco,0"
    Set-ItemProperty -Path "$base\shell\open\command" -Name '(default)' -Value "`"$destExe`""

    $progId = 'HKCU:\Software\Classes\NousHTML'
    New-Item -Path "$progId\DefaultIcon" -Force | Out-Null
    New-Item -Path "$progId\shell\open\command" -Force | Out-Null
    Set-ItemProperty -Path $progId -Name '(default)' -Value 'Nous HTML Document'
    Set-ItemProperty -Path "$progId\DefaultIcon" -Name '(default)' -Value "$destIco,0"
    Set-ItemProperty -Path "$progId\shell\open\command" -Name '(default)' -Value "`"$destExe`" `"%1`""

    New-Item -Path 'HKCU:\Software\RegisteredApplications' -Force | Out-Null
    Set-ItemProperty -Path 'HKCU:\Software\RegisteredApplications' -Name 'Nous' -Value 'Software\Clients\StartMenuInternet\Nous\Capabilities'
    Write-Host 'Registrado no Windows como navegador disponível em Configurações -> Aplicativos padrão.'
}

Write-Host ''
Write-Host "Nous pronto em: $destExe"
if (-not $NoLaunch) {
    Start-Process -FilePath $destExe
}

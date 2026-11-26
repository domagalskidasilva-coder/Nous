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

# Sync Gecko native XUL window icons (gecko/chrome/icons/default/) so Gecko applies Nous icon automatically
$repoIconsDir = Join-Path $workspace 'gecko\chrome\icons\default'
New-Item -ItemType Directory -Path $repoIconsDir -Force | Out-Null
foreach ($iconName in @('nous-window.ico', 'main-window.ico', 'default.ico')) {
    Copy-Item -LiteralPath $icoPath -Destination (Join-Path $repoIconsDir $iconName) -Force
}

# Helper: replace embedded Firefox fox RT_GROUP_ICON / RT_ICON resources in installed runtime\firefox.exe with nous.ico
function Set-ExeIconFromIco([string]$ExePath, [string]$IcoFile) {
    Add-Type -TypeDefinition @"
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;

public static class NousIconPatcher {
    private const int RT_ICON = 3;
    private const int RT_GROUP_ICON = 14;

    private delegate bool EnumResNameProc(IntPtr hModule, IntPtr lpszType, IntPtr lpszName, IntPtr lParam);
    private delegate bool EnumResLangProc(IntPtr hModule, IntPtr lpszType, IntPtr lpszName, ushort wLang, IntPtr lParam);

    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    private static extern IntPtr LoadLibraryEx(string lpFileName, IntPtr hFile, uint dwFlags);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool FreeLibrary(IntPtr hModule);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    private static extern bool EnumResourceNames(IntPtr hModule, IntPtr lpszType, EnumResNameProc cb, IntPtr lp);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    private static extern bool EnumResourceLanguages(IntPtr hModule, IntPtr lpszType, IntPtr lpszName, EnumResLangProc cb, IntPtr lp);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    private static extern IntPtr BeginUpdateResource(string pFileName, bool bDeleteExistingResources);
    [DllImport("kernel32.dll", SetLastError=true, CharSet=CharSet.Unicode)]
    private static extern bool UpdateResource(IntPtr hUpdate, IntPtr lpType, IntPtr lpName, ushort wLanguage, byte[] lpData, uint cbData);
    [DllImport("kernel32.dll", SetLastError=true)]
    private static extern bool EndUpdateResource(IntPtr hUpdate, bool fDiscard);

    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(int wEventId, uint uFlags, IntPtr dwItem1, IntPtr dwItem2);

    public static void ReplaceAllIcons(string exePath, string icoPath) {
        byte[] ico = File.ReadAllBytes(icoPath);
        int count = BitConverter.ToUInt16(ico, 4);
        var frames = new List<byte[]>();
        var groupMs = new MemoryStream();
        var bw = new BinaryWriter(groupMs);
        bw.Write((ushort)0);
        bw.Write((ushort)1);
        bw.Write((ushort)count);

        for (int i = 0; i < count; i++) {
            int off = 6 + i * 16;
            byte w = ico[off + 0];
            byte h = ico[off + 1];
            byte cc = ico[off + 2];
            byte res = ico[off + 3];
            ushort planes = BitConverter.ToUInt16(ico, off + 4);
            ushort bpp = BitConverter.ToUInt16(ico, off + 6);
            uint size = BitConverter.ToUInt32(ico, off + 8);
            uint imgOff = BitConverter.ToUInt32(ico, off + 12);

            byte[] frame = new byte[size];
            Buffer.BlockCopy(ico, (int)imgOff, frame, 0, (int)size);
            frames.Add(frame);

            bw.Write(w);
            bw.Write(h);
            bw.Write(cc);
            bw.Write(res);
            bw.Write(planes);
            bw.Write(bpp);
            bw.Write(size);
            bw.Write((ushort)(i + 1)); // RT_ICON id
        }
        bw.Flush();
        byte[] groupBytes = groupMs.ToArray();

        // Discover existing icon IDs & languages first
        var existingIcons = new List<Tuple<IntPtr, ushort>>();
        var existingGroups = new List<Tuple<IntPtr, ushort>>();
        IntPtr mod = LoadLibraryEx(exePath, IntPtr.Zero, 0x00000002);
        if (mod != IntPtr.Zero) {
            try {
                EnumResourceNames(mod, (IntPtr)RT_ICON, (m, t, name, lp) => {
                    EnumResourceLanguages(m, t, name, (m2, t2, n2, lang, lp2) => {
                        existingIcons.Add(Tuple.Create(n2, lang));
                        return true;
                    }, IntPtr.Zero);
                    return true;
                }, IntPtr.Zero);
                EnumResourceNames(mod, (IntPtr)RT_GROUP_ICON, (m, t, name, lp) => {
                    EnumResourceLanguages(m, t, name, (m2, t2, n2, lang, lp2) => {
                        existingGroups.Add(Tuple.Create(n2, lang));
                        return true;
                    }, IntPtr.Zero);
                    return true;
                }, IntPtr.Zero);
            } finally {
                FreeLibrary(mod);
            }
        }

        IntPtr hUpd = BeginUpdateResource(exePath, false);
        if (hUpd == IntPtr.Zero) throw new InvalidOperationException("BeginUpdateResource failed: " + Marshal.GetLastWin32Error());
        try {
            // Remove old Firefox fox icon frames & groups
            foreach (var item in existingIcons) UpdateResource(hUpd, (IntPtr)RT_ICON, item.Item1, item.Item2, null, 0);
            foreach (var item in existingGroups) UpdateResource(hUpd, (IntPtr)RT_GROUP_ICON, item.Item1, item.Item2, null, 0);

            // Write new Nous Kanso icon frames (#1..#count)
            for (int i = 0; i < frames.Count; i++) {
                UpdateResource(hUpd, (IntPtr)RT_ICON, (IntPtr)(i + 1), 1033, frames[i], (uint)frames[i].Length);
            }
            // Write all standard group icon IDs (#1, #32512 plus any original group IDs)
            var groupIds = new HashSet<long> { 1, 32512 };
            foreach (var g in existingGroups) {
                if ((long)g.Item1 < 0x10000) groupIds.Add((long)g.Item1);
            }
            foreach (long gid in groupIds) {
                UpdateResource(hUpd, (IntPtr)RT_GROUP_ICON, (IntPtr)gid, 1033, groupBytes, (uint)groupBytes.Length);
            }
        } catch {
            EndUpdateResource(hUpd, true);
            throw;
        }
        if (!EndUpdateResource(hUpd, false)) {
            throw new InvalidOperationException("EndUpdateResource failed: " + Marshal.GetLastWin32Error());
        }
    }

    [ComImport, Guid("00021401-0000-0000-C000-000000000046")]
    private class ShellLink {}

    [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("0000010b-0000-0000-C000-000000000046")]
    private interface IPersistFile {
        void GetClassID(out Guid pClassID);
        [PreserveSig] int IsDirty();
        void Load([MarshalAs(UnmanagedType.LPWStr)] string pszFileName, uint dwMode);
        void Save([MarshalAs(UnmanagedType.LPWStr)] string pszFileName, bool fRemember);
        void SaveCompleted([MarshalAs(UnmanagedType.LPWStr)] string pszFileName);
        void GetCurFile([MarshalAs(UnmanagedType.LPWStr)] out string ppszFileName);
    }

    [StructLayout(LayoutKind.Sequential, Pack = 4)]
    private struct PROPERTYKEY {
        public Guid fmtid;
        public uint pid;
    }

    [StructLayout(LayoutKind.Explicit)]
    private struct PROPVARIANT {
        [FieldOffset(0)] public ushort vt;
        [FieldOffset(8)] public IntPtr ptr;
        [FieldOffset(16)] public IntPtr pad;
    }

    [ComImport, InterfaceType(ComInterfaceType.InterfaceIsIUnknown), Guid("886D8EEB-8CF2-4446-8D02-CDBA1DBDCF99")]
    private interface IPropertyStore {
        void GetCount(out uint cProps);
        void GetAt(uint iProp, out PROPERTYKEY pkey);
        void GetValue(ref PROPERTYKEY key, out PROPVARIANT pv);
        void SetValue(ref PROPERTYKEY key, ref PROPVARIANT pv);
        void Commit();
    }

    public static void SetShortcutAppId(string lnkPath, string appId) {
        object link = new ShellLink();
        var pf = (IPersistFile)link;
        pf.Load(lnkPath, 2); // STGM_READWRITE
        var ps = (IPropertyStore)link;
        var key = new PROPERTYKEY { fmtid = new Guid("9F4C2855-9F79-4B39-A8D0-E1D42DE1D5F3"), pid = 5 };
        IntPtr pStr = Marshal.StringToCoTaskMemUni(appId);
        try {
            var pv = new PROPVARIANT { vt = 31, ptr = pStr };
            ps.SetValue(ref key, ref pv);
            ps.Commit();
            pf.Save(lnkPath, true);
        } finally {
            Marshal.FreeCoTaskMem(pStr);
            Marshal.ReleaseComObject(link);
        }
    }
}
"@
    [NousIconPatcher]::ReplaceAllIcons($ExePath, $IcoFile)
    [NousIconPatcher]::SHChangeNotify(0x08000000, 0, [IntPtr]::Zero, [IntPtr]::Zero)
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
$legacyGecko = Join-Path $InstallDir 'gecko'
$destRuntime = Join-Path $InstallDir 'runtime'
$destApp = Join-Path $destRuntime 'nous-app'
$destExe = Join-Path $InstallDir 'nous.exe'
$destIco = Join-Path $InstallDir 'nous.ico'

Copy-Item -LiteralPath $builtExe -Destination $destExe -Force
Copy-Item -LiteralPath $icoPath -Destination $destIco -Force

if (Test-Path -LiteralPath $legacyGecko) {
    Remove-Item -LiteralPath $legacyGecko -Recurse -Force
}

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
$excludeDirs = @('browser', 'uninstall', 'desktop-launcher', 'nous-app')

$robocopyArgs = @(
    $RuntimeSource, $destRuntime, '/MIR', '/NFL', '/NDL', '/NJH', '/NJS', '/NC', '/NS',
    '/XF'
) + $excludeFiles + @('/XD') + $excludeDirs
& robocopy @robocopyArgs | Out-Null
if ($LASTEXITCODE -ge 8) {
    throw "Robocopy falhou com código $LASTEXITCODE"
}

# Stage Nous chrome app INSIDE runtime\nous-app so Gecko's Windows content sandbox
# natively allows sandboxed content processes to load NousContentChild.sys.mjs.
if (Test-Path -LiteralPath $destApp) {
    Remove-Item -LiteralPath $destApp -Recurse -Force
}
Copy-Item -Path (Join-Path $workspace 'gecko') -Destination $destApp -Recurse -Force
# Ensure Windows AppContainer / Restricted Sandbox processes have Read-Execute on runtime\nous-app
& icacls $destRuntime /grant '*S-1-15-2-1:(OI)(CI)(RX)' '*S-1-15-2-2:(OI)(CI)(RX)' /T /Q 2>$null | Out-Null

# Replace Firefox fox icon inside installed runtime\firefox.exe with Nous Kanso icon
$destFirefoxExe = Join-Path $destRuntime 'firefox.exe'
$destFirefoxSig = Join-Path $destRuntime 'firefox.exe.sig'
if (Test-Path -LiteralPath $destFirefoxSig) { Remove-Item -LiteralPath $destFirefoxSig -Force -ErrorAction SilentlyContinue }
Write-Host 'Aplicando ícone Kanso do Nous no executável do motor (barra de tarefas)...'
Set-ExeIconFromIco $destFirefoxExe $icoPath

# Flush Gecko startupCache in user profile (never touches cookies, logins, bookmarks or history)
$userStartupCache = Join-Path $env:LOCALAPPDATA 'Nous\Profile\startupCache'
if (Test-Path -LiteralPath $userStartupCache) {
    Remove-Item -LiteralPath $userStartupCache -Recurse -Force -ErrorAction SilentlyContinue
}
# Ask Windows Explorer shell to refresh icon cache immediately
Start-Process -FilePath 'ie4uinit.exe' -ArgumentList '-show' -WindowStyle Hidden -ErrorAction SilentlyContinue | Out-Null

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
    try { [NousIconPatcher]::SetShortcutAppId($lnkPath, 'Nous.Browser.Desktop') } catch {}
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

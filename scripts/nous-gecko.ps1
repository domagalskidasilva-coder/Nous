param(
    [string] $Url = '',
    [string] $FirefoxPath = '',
    [string] $ProfilePath = '',
    [switch] $CleanProfile
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$application = Join-Path $workspace 'gecko\application.ini'
$runtime = Join-Path $workspace 'target\nous-gecko-runtime'

if (-not $FirefoxPath) {
    $candidates = @(
        (Join-Path $runtime 'core\firefox.exe'),
        "$env:ProgramFiles\Mozilla Firefox\firefox.exe",
        "${env:ProgramFiles(x86)}\Mozilla Firefox\firefox.exe"
    )
    $FirefoxPath = $candidates | Where-Object { Test-Path -LiteralPath $_ -PathType Leaf } | Select-Object -First 1
}
if (-not $FirefoxPath -or -not (Test-Path -LiteralPath $FirefoxPath -PathType Leaf)) {
    throw 'Informe -FirefoxPath com o caminho do firefox.exe do Firefox 153 ESR.'
}

$firefox = Get-Item -LiteralPath $FirefoxPath
if ($firefox.VersionInfo.FileMajorPart -ne 153) {
    throw "O Nous requer o runtime Firefox 153 ESR (encontrado: $($firefox.VersionInfo.FileVersion))."
}

$profileDir = if ($ProfilePath) { $ProfilePath } else { Join-Path $runtime 'nous-profile' }
if ($CleanProfile -and (Test-Path -LiteralPath $profileDir)) {
    Remove-Item -LiteralPath $profileDir -Recurse -Force
}
New-Item -ItemType Directory -Path $profileDir -Force | Out-Null

$arguments = '-app "{0}" -no-remote -profile "{1}"' -f $application, $profileDir

$start = New-Object System.Diagnostics.ProcessStartInfo
$start.FileName = $firefox.FullName
$start.Arguments = $arguments
$start.WorkingDirectory = $workspace
$start.UseShellExecute = $false
$start.EnvironmentVariables.Remove('XUL_APP_FILE')
$start.EnvironmentVariables['NOUS_GECKO_START_URL'] = $Url
$start.EnvironmentVariables['MOZ_CRASHREPORTER_DISABLE'] = '1'
$start.EnvironmentVariables['MOZ_DISABLE_AUTO_SAFE_MODE'] = '1'
$start.EnvironmentVariables['MOZ_DISABLE_CONTENT_SANDBOX'] = ''
$start.EnvironmentVariables.Remove('MOZ_DISABLE_CONTENT_SANDBOX')

[System.Diagnostics.Process]::Start($start)

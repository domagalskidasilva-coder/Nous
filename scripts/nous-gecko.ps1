param(
    [string] $Url = '',
    [string] $FirefoxPath = ''
)

$ErrorActionPreference = 'Stop'
$workspace = Split-Path $PSScriptRoot -Parent
$application = Join-Path $workspace 'gecko\application.ini'
$runtime = Join-Path $workspace 'target\nous-gecko-runtime'

if (-not $FirefoxPath) {
    $FirefoxPath = Join-Path $runtime 'core\firefox.exe'
}
if (-not (Test-Path -LiteralPath $FirefoxPath -PathType Leaf)) {
    throw 'Informe -FirefoxPath com o caminho do firefox.exe do Firefox 153 ESR.'
}
$firefox = Get-Item -LiteralPath $FirefoxPath
if ($firefox.VersionInfo.FileMajorPart -ne 153) {
    throw 'Esta prévia usa o Firefox 153 ESR.'
}
$profileDir = Join-Path $runtime 'nous-profile'
New-Item -ItemType Directory -Path $profileDir -Force | Out-Null
$arguments = '-app "{0}" -no-remote -profile "{1}"' -f $application, $profileDir

$start = New-Object System.Diagnostics.ProcessStartInfo
$start.FileName = $firefox.FullName
$start.Arguments = $arguments
$start.WorkingDirectory = $workspace
$start.UseShellExecute = $false
$start.EnvironmentVariables.Remove('XUL_APP_FILE')
$start.EnvironmentVariables['NOUS_GECKO_START_URL'] = $Url
[System.Diagnostics.Process]::Start($start)

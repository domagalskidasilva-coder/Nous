<#
.SYNOPSIS
  Roda o cargo dentro do ambiente do Visual Studio Build Tools (Windows).

.DESCRIPTION
  Útil quando o terminal não é um "Developer PowerShell" ou quando o Build Tools foi
  instalado só com o compilador x86→x64 (sem a pasta bin\Hostx64): o Rust não acha o
  linker sozinho nesses casos. Aceita os mesmos argumentos do cargo.

.EXAMPLE
  .\scripts\cargo.ps1 run --release
  .\scripts\cargo.ps1 test --workspace
#>
$ErrorActionPreference = "Stop"
$vswhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
if (-not (Test-Path $vswhere)) { throw "Visual Studio Build Tools não encontrado (instale com o componente C++)." }
$vs = & $vswhere -all -latest -products * -property installationPath | Select-Object -First 1
$build = Join-Path $vs "VC\Auxiliary\Build"
$hasX64 = Get-ChildItem (Join-Path $vs "VC\Tools\MSVC\*\bin\Hostx64\x64\cl.exe") -ErrorAction SilentlyContinue
$bat = if ($hasX64) { Join-Path $build "vcvarsall.bat"; $arch = "x64" } else { Join-Path $build "vcvarsx86_amd64.bat"; $arch = "" }

$dump = cmd /c "call `"$bat`" $arch >nul 2>&1 && set"
foreach ($line in $dump) {
  if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($matches[1], $matches[2]) }
}

# Windows SDK recém-instalado nem sempre aparece no vcvars: acrescenta à mão se faltar.
$kit = "${env:ProgramFiles(x86)}\Windows Kits\10"
$ver = Get-ChildItem "$kit\Lib" -Directory -ErrorAction SilentlyContinue |
  Where-Object { Test-Path "$($_.FullName)\um\x64\kernel32.lib" } |
  Sort-Object Name | Select-Object -Last 1 -ExpandProperty Name
if ($ver -and ($env:LIB -notmatch [regex]::Escape("$ver\um\x64"))) {
  $env:LIB = "$kit\Lib\$ver\um\x64;$kit\Lib\$ver\ucrt\x64;$env:LIB"
  $env:INCLUDE = "$kit\Include\$ver\ucrt;$kit\Include\$ver\um;$kit\Include\$ver\shared;$env:INCLUDE"
  $env:PATH = "$kit\bin\$ver\x64;$env:PATH"
}
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

Set-Location (Split-Path $PSScriptRoot -Parent)
& cargo @args
exit $LASTEXITCODE

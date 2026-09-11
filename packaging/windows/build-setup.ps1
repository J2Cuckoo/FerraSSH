param(
  [Parameter(Mandatory = $true)][string]$Root,
  [Parameter(Mandatory = $true)][string]$Exe
)

$ErrorActionPreference = "Stop"
$pack = Join-Path $Root "packaging\windows"
$outDir = Join-Path $Root "dist\windows"
$setupName = "FerraSSH-0.1.0-x64-Setup.exe"
$isccCandidates = @(
  (Join-Path $Root "tools\innosetup\ISCC.exe"),
  "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
  "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
  "$env:ProgramFiles\Inno Setup 6\ISCC.exe"
)
$iscc = $isccCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
if (-not $iscc) {
  Write-Host "Installing Inno Setup compiler..."
  winget install --id JRSoftware.InnoSetup -e --accept-package-agreements --accept-source-agreements --disable-interactivity
  $iscc = $isccCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) {
  throw "ISCC.exe not found. Install Inno Setup 6."
}

New-Item -ItemType Directory -Force -Path $outDir, (Join-Path $pack "fonts") | Out-Null
Copy-Item -Force $Exe (Join-Path $pack "FerraSSH.exe")

Get-ChildItem $outDir -File -ErrorAction SilentlyContinue | Remove-Item -Force
$p = Start-Process -FilePath $iscc -ArgumentList @("/Q", (Join-Path $pack "ferrassh.iss")) -Wait -PassThru -WorkingDirectory $pack
if ($p.ExitCode -ne 0) {
  throw "ISCC failed with exit $($p.ExitCode)"
}

$setup = Join-Path $outDir $setupName
if (-not (Test-Path $setup)) {
  throw "Installer was not created: $setup"
}

Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $outDir "FerraSSH.exe")
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue (Join-Path $outDir "stage")
Write-Host "installer=$setup"

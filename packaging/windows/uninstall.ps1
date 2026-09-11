$ErrorActionPreference = "Stop"
$dest = Join-Path $env:LOCALAPPDATA "Programs\FerraSSH"
$lnk = Join-Path ([Environment]::GetFolderPath("Programs")) "FerraSSH.lnk"
Remove-Item -Force -ErrorAction SilentlyContinue $lnk
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $dest
Remove-Item -Recurse -Force -ErrorAction SilentlyContinue "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\FerraSSH"
Write-Host "FerraSSH removed."

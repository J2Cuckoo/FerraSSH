$ErrorActionPreference = "Stop"
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$dest = Join-Path $env:LOCALAPPDATA "Programs\FerraSSH"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
Copy-Item -Force (Join-Path $here "FerraSSH.exe") (Join-Path $dest "FerraSSH.exe")
Copy-Item -Force (Join-Path $here "uninstall.ps1") (Join-Path $dest "uninstall.ps1")

$wsh = New-Object -ComObject WScript.Shell
$programs = [Environment]::GetFolderPath("Programs")
$shortcut = $wsh.CreateShortcut((Join-Path $programs "FerraSSH.lnk"))
$shortcut.TargetPath = Join-Path $dest "FerraSSH.exe"
$shortcut.WorkingDirectory = $dest
$shortcut.Description = "FerraSSH"
$shortcut.Save()

$uninst = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\FerraSSH"
New-Item -Path $uninst -Force | Out-Null
New-ItemProperty -Path $uninst -Name DisplayName -Value "FerraSSH" -PropertyType String -Force | Out-Null
New-ItemProperty -Path $uninst -Name DisplayVersion -Value "0.1.0" -PropertyType String -Force | Out-Null
New-ItemProperty -Path $uninst -Name Publisher -Value "Guizhou Lixian Network Technology Co., Ltd." -PropertyType String -Force | Out-Null
New-ItemProperty -Path $uninst -Name InstallLocation -Value $dest -PropertyType String -Force | Out-Null
New-ItemProperty -Path $uninst -Name DisplayIcon -Value (Join-Path $dest "FerraSSH.exe") -PropertyType String -Force | Out-Null
New-ItemProperty -Path $uninst -Name UninstallString -Value "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$dest\uninstall.ps1`"" -PropertyType String -Force | Out-Null
New-ItemProperty -Path $uninst -Name NoModify -Value 1 -PropertyType DWord -Force | Out-Null
New-ItemProperty -Path $uninst -Name NoRepair -Value 1 -PropertyType DWord -Force | Out-Null

Write-Host "FerraSSH installed to $dest"

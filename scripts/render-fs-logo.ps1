param(
  [Parameter(Mandatory = $true)][string]$OutDir,
  [int[]]$Sizes = @(16, 24, 32, 48, 64, 128, 256)
)

$ErrorActionPreference = "Stop"
Add-Type -AssemblyName System.Drawing
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$bg = [System.Drawing.Color]::FromArgb(255, 61, 205, 195)
$fg = [System.Drawing.Color]::FromArgb(255, 6, 32, 30)

function New-RoundRect([int]$w, [int]$h, [int]$r) {
  if ($r -lt 1) { $r = 1 }
  $d = [Math]::Min($r * 2, [Math]::Min($w, $h))
  $path = New-Object System.Drawing.Drawing2D.GraphicsPath
  $path.AddArc(0, 0, $d, $d, 180, 90)
  $path.AddArc($w - $d, 0, $d, $d, 270, 90)
  $path.AddArc($w - $d, $h - $d, $d, $d, 0, 90)
  $path.AddArc(0, $h - $d, $d, $d, 90, 90)
  $path.CloseFigure()
  return $path
}

function New-LogoBitmap([int]$size) {
  $bmp = New-Object System.Drawing.Bitmap $size, $size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $g = [System.Drawing.Graphics]::FromImage($bmp)
  $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
  $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
  $g.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
  $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
  $g.Clear([System.Drawing.Color]::FromArgb(0, 0, 0, 0))

  $radius = if ($size -le 24) { [Math]::Max(2, [int][Math]::Round($size * 0.14)) } else { [Math]::Max(2, [int][Math]::Round($size * 0.1875)) }
  $path = New-RoundRect $size $size $radius
  $fill = New-Object System.Drawing.SolidBrush $bg
  $g.FillPath($fill, $path)

  $fontPx = if ($size -le 24) { [Math]::Max(8, [int][Math]::Round($size * 0.52)) } else { [Math]::Max(7, [int][Math]::Round($size * 0.44)) }
  $font = $null
  foreach ($name in @("Segoe UI", "Microsoft YaHei UI", "Microsoft YaHei", "Arial")) {
    try {
      $font = New-Object System.Drawing.Font $name, $fontPx, ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
      break
    } catch {
      $font = $null
    }
  }
  if (-not $font) {
    $font = New-Object System.Drawing.Font ([System.Drawing.FontFamily]::GenericSansSerif), $fontPx, ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
  }

  $textBrush = New-Object System.Drawing.SolidBrush $fg
  $sf = New-Object System.Drawing.StringFormat
  $sf.Alignment = [System.Drawing.StringAlignment]::Center
  $sf.LineAlignment = [System.Drawing.StringAlignment]::Center
  $sf.FormatFlags = [System.Drawing.StringFormatFlags]::NoWrap
  $box = New-Object System.Drawing.RectangleF 0, (-$size * 0.03), $size, $size
  $g.DrawString("Fs", $font, $textBrush, $box, $sf)

  $g.Dispose()
  $fill.Dispose()
  $textBrush.Dispose()
  $font.Dispose()
  $path.Dispose()
  return $bmp
}

function Get-IcoImage([System.Drawing.Bitmap]$bmp) {
  $w = $bmp.Width
  $h = $bmp.Height
  $rect = New-Object System.Drawing.Rectangle 0, 0, $w, $h
  $data = $bmp.LockBits($rect, [System.Drawing.Imaging.ImageLockMode]::ReadOnly, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
  $top = New-Object byte[] ($data.Stride * $h)
  [Runtime.InteropServices.Marshal]::Copy($data.Scan0, $top, 0, $top.Length)
  $bmp.UnlockBits($data)

  $xor = New-Object byte[] ($w * $h * 4)
  for ($y = 0; $y -lt $h; $y++) {
    [Buffer]::BlockCopy($top, ($h - 1 - $y) * $data.Stride, $xor, $y * $w * 4, $w * 4)
  }

  $andStride = [Math]::Max(4, [int][Math]::Ceiling($w / 32.0) * 4)
  $and = New-Object byte[] ($andStride * $h)

  $header = New-Object byte[] 40
  [BitConverter]::GetBytes([int]40).CopyTo($header, 0)
  [BitConverter]::GetBytes([int]$w).CopyTo($header, 4)
  [BitConverter]::GetBytes([int]($h * 2)).CopyTo($header, 8)
  [BitConverter]::GetBytes([int16]1).CopyTo($header, 12)
  [BitConverter]::GetBytes([int16]32).CopyTo($header, 14)
  [BitConverter]::GetBytes([int]($xor.Length)).CopyTo($header, 20)
  $all = New-Object byte[] ($header.Length + $xor.Length + $and.Length)
  [Buffer]::BlockCopy($header, 0, $all, 0, $header.Length)
  [Buffer]::BlockCopy($xor, 0, $all, $header.Length, $xor.Length)
  [Buffer]::BlockCopy($and, 0, $all, $header.Length + $xor.Length, $and.Length)
  return ,$all
}

function Write-ClassicIco([System.Drawing.Bitmap[]]$bitmaps, [string]$path) {
  $images = @()
  foreach ($bmp in $bitmaps) { $images += ,(Get-IcoImage $bmp) }
  $count = $images.Count
  $dir = New-Object byte[] (6 + 16 * $count)
  $dir[2] = 1
  [BitConverter]::GetBytes([int16]$count).CopyTo($dir, 4)
  $offset = $dir.Length
  for ($i = 0; $i -lt $count; $i++) {
    $bmp = $bitmaps[$i]
    $img = $images[$i]
    $o = 6 + $i * 16
    $dir[$o] = if ($bmp.Width -ge 256) { 0 } else { [byte]$bmp.Width }
    $dir[$o + 1] = if ($bmp.Height -ge 256) { 0 } else { [byte]$bmp.Height }
    [BitConverter]::GetBytes([int16]1).CopyTo($dir, $o + 4)
    [BitConverter]::GetBytes([int16]32).CopyTo($dir, $o + 6)
    [BitConverter]::GetBytes([int]$img.Length).CopyTo($dir, $o + 8)
    [BitConverter]::GetBytes([int]$offset).CopyTo($dir, $o + 12)
    $offset += $img.Length
  }
  $fs = [IO.File]::Create($path)
  $fs.Write($dir, 0, $dir.Length)
  foreach ($img in $images) { $fs.Write($img, 0, $img.Length) }
  $fs.Close()
}

$bitmaps = @()
foreach ($size in $Sizes) {
  $bmp = New-LogoBitmap $size
  $bmp.Save((Join-Path $OutDir ("{0}x{0}.png" -f $size)), [System.Drawing.Imaging.ImageFormat]::Png)
  $bitmaps += $bmp
}

$icoSizes = @(16, 24, 32, 48, 256)
$icoBmps = @()
foreach ($size in $icoSizes) {
  $found = $bitmaps | Where-Object { $_.Width -eq $size } | Select-Object -First 1
  if ($found) { $icoBmps += $found }
}
Write-ClassicIco $icoBmps (Join-Path $OutDir "icon.ico")

foreach ($bmp in $bitmaps) { $bmp.Dispose() }

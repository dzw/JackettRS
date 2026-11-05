Add-Type -AssemblyName System.Drawing

$dir = "F:/github/JackettRS/jackett-desktop/icons"
New-Item -ItemType Directory -Force -Path $dir | Out-Null

$bmp = New-Object System.Drawing.Bitmap(256, 256)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.Clear([System.Drawing.Color]::FromArgb(61, 156, 255))
$font = New-Object System.Drawing.Font("Segoe UI", 90, [System.Drawing.FontStyle]::Bold)
$sf = New-Object System.Drawing.StringFormat
$sf.Alignment = [System.Drawing.StringAlignment]::Center
$sf.LineAlignment = [System.Drawing.StringAlignment]::Center
$rect = New-Object System.Drawing.RectangleF(0, 0, 256, 256)
$g.DrawString("J", $font, [System.Drawing.Brushes]::White, $rect, $sf)
$g.Dispose()

foreach ($s in @("32x32", "128x128", "128x128@2x", "icon")) {
    $sz = 32
    if ($s -eq "128x128") { $sz = 128 }
    if ($s -eq "128x128@2x" -or $s -eq "icon") { $sz = 256 }
    $b = New-Object System.Drawing.Bitmap($sz, $sz)
    $gg = [System.Drawing.Graphics]::FromImage($b)
    $gg.InterpolationMode = "HighQualityBicubic"
    $gg.DrawImage($bmp, 0, 0, $sz, $sz)
    $gg.Dispose()
    $b.Save("$dir/$s.png", [System.Drawing.Imaging.ImageFormat]::Png)
    $b.Dispose()
}

$ico = [System.Drawing.Icon]::FromHandle($bmp.GetHicon())
$fs = [System.IO.File]::Create("$dir/icon.ico")
$ico.Save($fs)
$fs.Close()
$bmp.Dispose()
Write-Output "ICONS-OK"

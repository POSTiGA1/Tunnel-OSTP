Add-Type -AssemblyName System.Drawing
$imgPath = "D:\ospab-projects\ostp\ostp-flutter\android_icon.png"
$backupPath = "D:\ospab-projects\ostp\ostp-flutter\android_icon_backup.png"

# Copy as backup
Copy-Item -Path $imgPath -Destination $backupPath -Force

# Load image
$srcImg = [System.Drawing.Image]::FromFile($imgPath)
$w = $srcImg.Width
$h = $srcImg.Height

# Create new transparent bitmap of the SAME size
$bmp = New-Object System.Drawing.Bitmap($w, $h)
$bmp.MakeTransparent()
$graphics = [System.Drawing.Graphics]::FromImage($bmp)

# Set high quality resizing
$graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
$graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality

# Clear with transparent
$graphics.Clear([System.Drawing.Color]::Transparent)

# We want to scale to 50% and center
$newW = [math]::Round($w / 2)
$newH = [math]::Round($h / 2)
$x = [math]::Round(($w - $newW) / 2)
$y = [math]::Round(($h - $newH) / 2)

# Draw image scaled down
$rect = New-Object System.Drawing.Rectangle($x, $y, $newW, $newH)
$graphics.DrawImage($srcImg, $rect)

# Dispose graphics and src before saving over
$graphics.Dispose()
$srcImg.Dispose()

# Save over original
$bmp.Save($imgPath, [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()

Write-Output "Successfully resized image."

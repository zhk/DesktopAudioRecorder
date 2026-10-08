# 把 src/download.png 转成多尺寸 src/icon.ico（PNG 压缩格式，Vista+ 支持）
# 用法: pwsh make-icon.ps1 [源图片] [输出ico]
param(
    [string]$Source = "$PSScriptRoot\src\download.png",
    [string]$Dest = "$PSScriptRoot\src\icon.ico"
)

Add-Type -AssemblyName System.Drawing

$sizes = @(16, 24, 32, 48, 64, 128, 256)
$src = [System.Drawing.Image]::FromFile($Source)

try {
    $pngs = New-Object 'System.Collections.Generic.List[byte[]]'
    foreach ($s in $sizes) {
        $bmp = New-Object System.Drawing.Bitmap $s, $s
        $g = [System.Drawing.Graphics]::FromImage($bmp)
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
        $g.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $g.DrawImage($src, 0, 0, $s, $s)
        $g.Dispose()
        $ms = New-Object System.IO.MemoryStream
        $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
        $pngs.Add($ms.ToArray())
        $ms.Dispose()
        $bmp.Dispose()
    }

    $fs = [System.IO.File]::Create($Dest)
    $bw = New-Object System.IO.BinaryWriter($fs)
    $bw.Write([uint16]0)              # reserved
    $bw.Write([uint16]1)              # type = icon
    $bw.Write([uint16]$sizes.Count)   # image count
    $offset = 6 + 16 * $sizes.Count
    for ($i = 0; $i -lt $sizes.Count; $i++) {
        $dim = [byte]($sizes[$i] -band 0xFF)   # 256 记为 0
        $bw.Write($dim)                # width
        $bw.Write($dim)                # height
        $bw.Write([byte]0)             # palette
        $bw.Write([byte]0)             # reserved
        $bw.Write([uint16]1)           # planes
        $bw.Write([uint16]32)          # bpp
        $bw.Write([uint32]$pngs[$i].Length)
        $bw.Write([uint32]$offset)
        $offset += $pngs[$i].Length
    }
    foreach ($p in $pngs) { $bw.Write($p) }
    $bw.Close()
    Write-Output "written: $Dest ($($pngs.Count) sizes, $((Get-Item $Dest).Length) bytes)"
} finally {
    $src.Dispose()
}

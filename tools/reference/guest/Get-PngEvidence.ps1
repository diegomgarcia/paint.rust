$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
Add-Type -AssemblyName System.Drawing
$probeSource = @"
using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Security.Cryptography;

public sealed class PngProbeResult {
    public int Width;
    public int Height;
    public int DecodedRgbaBytes;
    public int PixelCount;
    public int RowStrideBytes;
    public string RgbaSha256;
    public int[] FirstPixelRgba;
    public bool AllPixelsEqual;
    public int UniqueColorCount;
    public string Decoder;
}

public static class PngProbe {
    public static PngProbeResult Compute(string path) {
        using (var source = new Bitmap(path))
        using (var bitmap = source.Clone(new Rectangle(0, 0, source.Width, source.Height), PixelFormat.Format32bppArgb)) {
            int width = bitmap.Width;
            int height = bitmap.Height;
            if ((long)width * height > 16000000L) throw new InvalidOperationException("pixel bound exceeded");
            var rectangle = new Rectangle(0, 0, width, height);
            var data = bitmap.LockBits(rectangle, ImageLockMode.ReadOnly, PixelFormat.Format32bppArgb);
            try {
                if (data.Stride < width * 4) throw new InvalidOperationException("unexpected bitmap stride");
                var rgba = new byte[checked(width * height * 4)];
                var row = new byte[data.Stride];
                var colors = new HashSet<uint>();
                bool allEqual = true;
                uint first = 0;
                int outputOffset = 0;
                for (int y = 0; y < height; y++) {
                    Marshal.Copy(IntPtr.Add(data.Scan0, y * data.Stride), row, 0, width * 4);
                    for (int x = 0; x < width; x++) {
                        int inputOffset = x * 4;
                        byte blue = row[inputOffset];
                        byte green = row[inputOffset + 1];
                        byte red = row[inputOffset + 2];
                        byte alpha = row[inputOffset + 3];
                        rgba[outputOffset] = red;
                        rgba[outputOffset + 1] = green;
                        rgba[outputOffset + 2] = blue;
                        rgba[outputOffset + 3] = alpha;
                        uint packed = ((uint)red << 24) | ((uint)green << 16) | ((uint)blue << 8) | alpha;
                        if (outputOffset == 0) first = packed;
                        else if (packed != first) allEqual = false;
                        if (colors.Count <= 256) colors.Add(packed);
                        outputOffset += 4;
                    }
                }
                string hash;
                using (var sha = SHA256.Create()) hash = BitConverter.ToString(sha.ComputeHash(rgba)).Replace("-", "").ToLowerInvariant();
                return new PngProbeResult {
                    Width = width,
                    Height = height,
                    DecodedRgbaBytes = rgba.Length,
                    PixelCount = width * height,
                    RowStrideBytes = width * 4,
                    RgbaSha256 = hash,
                    FirstPixelRgba = new int[] { (int)(first >> 24), (int)((first >> 16) & 255), (int)((first >> 8) & 255), (int)(first & 255) },
                    AllPixelsEqual = allEqual,
                    UniqueColorCount = colors.Count > 256 ? -1 : colors.Count,
                    Decoder = "guest PowerShell System.Drawing Bitmap clone to Format32bppArgb; BGRA memory converted to RGBA8"
                };
            } finally {
                bitmap.UnlockBits(data);
            }
        }
    }
}
"@
Add-Type -TypeDefinition $probeSource -ReferencedAssemblies System.Drawing
$file = Get-Item -LiteralPath $Path
$encodedHash = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
$probe = [PngProbe]::Compute($file.FullName)
[pscustomobject]@{
    path = $file.FullName
    encoded_bytes = $file.Length
    sha256 = $encodedHash
    width = $probe.Width
    height = $probe.Height
    decoded_rgba_bytes = $probe.DecodedRgbaBytes
    pixel_count = $probe.PixelCount
    row_stride_bytes = $probe.RowStrideBytes
    rgba_sha256 = $probe.RgbaSha256
    first_pixel_rgba = $probe.FirstPixelRgba
    all_pixels_equal = $probe.AllPixelsEqual
    unique_color_count = $probe.UniqueColorCount
    decoder = $probe.Decoder
} | ConvertTo-Json -Depth 4 -Compress

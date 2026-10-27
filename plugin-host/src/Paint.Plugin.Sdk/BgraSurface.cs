namespace Paint.Plugin.Sdk;

/// <summary>
/// A copied, tightly packed BGRA8 surface. Rows are top-down and Stride is Width * 4.
/// Alpha is straight (not premultiplied). Pixels are mutable only inside this owned copy.
/// </summary>
public sealed class BgraSurface
{
    public const int MaxPixelBytes = 64 * 1024 * 1024;

    public BgraSurface(int width, int height, byte[] pixels)
    {
        ArgumentNullException.ThrowIfNull(pixels);
        ArgumentOutOfRangeException.ThrowIfNegativeOrZero(width);
        ArgumentOutOfRangeException.ThrowIfNegativeOrZero(height);
        long expected = checked((long)width * height * 4);
        if (expected > MaxPixelBytes)
        {
            throw new ArgumentOutOfRangeException(
                nameof(pixels),
                $"The {expected}-byte surface exceeds the {MaxPixelBytes}-byte bound.");
        }
        if (pixels.Length != expected)
        {
            throw new ArgumentException(
                $"A {width}x{height} BGRA surface requires exactly {expected} bytes, got {pixels.Length}.",
                nameof(pixels));
        }

        Width = width;
        Height = height;
        Stride = checked(width * 4);
        Pixels = (byte[])pixels.Clone();
    }

    public int Width { get; }

    public int Height { get; }

    public int Stride { get; }

    public byte[] Pixels { get; }
}

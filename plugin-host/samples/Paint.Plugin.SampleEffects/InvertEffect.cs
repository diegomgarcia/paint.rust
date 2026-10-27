using Paint.Plugin.Sdk;

namespace Paint.Plugin.SampleEffects;

/// <summary>A source-compiled transport sample, not a Paint.NET binary compatibility test.</summary>
public sealed class InvertEffect : ICloneEffect
{
    public string Name => "Invert";

    public void Apply(BgraSurface surface, CancellationToken cancellationToken)
    {
        ArgumentNullException.ThrowIfNull(surface);
        byte[] pixels = surface.Pixels;
        for (int offset = 0; offset < pixels.Length; offset += 4)
        {
            if ((offset & 0x3fff) == 0)
            {
                cancellationToken.ThrowIfCancellationRequested();
            }
            pixels[offset] = (byte)(255 - pixels[offset]);
            pixels[offset + 1] = (byte)(255 - pixels[offset + 1]);
            pixels[offset + 2] = (byte)(255 - pixels[offset + 2]);
            // Byte 3 is alpha and is deliberately not changed.
        }
    }
}

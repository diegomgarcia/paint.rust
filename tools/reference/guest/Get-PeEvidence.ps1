if ([string]::IsNullOrEmpty($Path)) { throw 'Path is required' }
$item = Get-Item -LiteralPath $Path
if ($item.PSIsContainer) { throw 'Path is a directory' }
$stream = [System.IO.File]::OpenRead($item.FullName)
$reader = New-Object System.IO.BinaryReader($stream)
try {
    if ($reader.ReadUInt16() -ne 0x5A4D) { throw 'Missing MZ signature' }
    $stream.Position = 0x3C
    $peOffset = $reader.ReadUInt32()
    if ($peOffset -lt 0x40 -or $peOffset -gt ($stream.Length - 24)) { throw 'Invalid PE offset' }
    $stream.Position = $peOffset
    if ($reader.ReadUInt32() -ne 0x00004550) { throw 'Missing PE signature' }
    $machine = $reader.ReadUInt16()
} finally {
    $reader.Dispose()
    $stream.Dispose()
}
$architecture = switch ($machine) {
    0xAA64 { 'ARM64' }
    0x8664 { 'x64' }
    0x014C { 'x86' }
    0x01C4 { 'ARMv7' }
    default { 'unknown' }
}
[pscustomobject]@{
    path = $item.FullName
    length = [int64]$item.Length
    sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    pe_offset = [uint32]$peOffset
    machine_hex = ('0x{0:X4}' -f [int]$machine)
    architecture = $architecture
    inspection_boundary = 'PE machine header only; no disassembly, REA, IL inspection, or decompilation.'
} | ConvertTo-Json -Compress

if ([string]::IsNullOrEmpty($SourceRoot)) {
    $SourceRoot = 'C:\PaintR-Reference'
}

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

$names = @(
    'dsh', 'deepseek', 'deepseek-harness', 'harness', 'paint-r',
    'node', 'npm', 'npx', 'pnpm', 'git', 'rustc', 'cargo', 'rustup',
    'dotnet', 'python', 'python3', 'clang', 'cmake', 'ninja', 'winget', 'curl', 'tar'
)
$commands = foreach ($name in $names) {
    $resolved = @(Get-Command -Name $name -All -ErrorAction SilentlyContinue)
    if ($resolved.Count -eq 0) {
        [pscustomobject]@{ name = $name; found = $false; command_type = $null; source = $null; version = $null }
    } else {
        foreach ($item in $resolved) {
            [pscustomobject]@{
                name = $name
                found = $true
                command_type = [string]$item.CommandType
                source = [string]$item.Source
                version = if ($null -eq $item.Version) { $null } else { [string]$item.Version }
            }
        }
    }
}

$paths = @(
    'C:\Users\CodeFreak\AppData\Roaming\npm\dsh.cmd',
    'C:\Users\CodeFreak\AppData\Roaming\npm\dsh.ps1',
    'C:\Users\CodeFreak\AppData\Roaming\npm\deepseek-harness.cmd',
    'C:\Users\CodeFreak\.local\bin\dsh.exe',
    'C:\Users\CodeFreak\.cargo\bin\dsh.exe',
    'C:\Users\CodeFreak\.cargo\bin\cargo.exe',
    'C:\Users\CodeFreak\.cargo\bin\rustc.exe',
    'C:\Users\CodeFreak\.cargo\bin\rustup.exe',
    'C:\Users\CodeFreak\.rustup\toolchains',
    'C:\Program Files\nodejs\node.exe',
    'C:\Program Files (x86)\nodejs\node.exe',
    'C:\Program Files\dotnet\dotnet.exe',
    'C:\Program Files\Git\cmd\git.exe',
    'C:\PaintR-Reference',
    'C:\Users\CodeFreak\Projects',
    'C:\Users\CodeFreak\source'
)
$pathInventory = foreach ($path in $paths) {
    $item = Get-Item -LiteralPath $path -ErrorAction SilentlyContinue
    [pscustomobject]@{
        path = $path
        exists = ($null -ne $item)
        is_directory = if ($null -eq $item) { $null } else { [bool]$item.PSIsContainer }
        length = if ($null -eq $item -or $item.PSIsContainer) { $null } else { [int64]$item.Length }
        file_version = if ($null -eq $item -or $item.PSIsContainer) { $null } else { [string]$item.VersionInfo.FileVersion }
    }
}

$os = Get-CimInstance Win32_OperatingSystem
$computer = Get-CimInstance Win32_ComputerSystem
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$disks = @(Get-CimInstance Win32_LogicalDisk -Filter 'DriveType=3' | ForEach-Object {
    [pscustomobject]@{
        device_id = $_.DeviceID
        size_bytes = [uint64]$_.Size
        free_bytes = [uint64]$_.FreeSpace
        file_system = [string]$_.FileSystem
    }
})
[pscustomobject]@{
    schema_version = 1
    kind = 'harness-and-native-build-inventory'
    collected_at = [DateTimeOffset]::Now.ToString('o')
    native_system_type = [string]$computer.SystemType
    processor_architecture_code = [int]$cpu.Architecture
    processor_address_width = [int]$cpu.AddressWidth
    qga_process_architecture = [string]$env:PROCESSOR_ARCHITECTURE
    os_caption = [string]$os.Caption
    os_version = [string]$os.Version
    os_build = [string]$os.BuildNumber
    total_memory_bytes = [uint64]$computer.TotalPhysicalMemory
    free_memory_bytes = [uint64]$os.FreePhysicalMemory * 1024
    logical_processors = [int]$computer.NumberOfLogicalProcessors
    source_root = $SourceRoot
    source_root_exists = [bool](Test-Path -LiteralPath $SourceRoot)
    commands = @($commands)
    explicit_paths = @($pathInventory)
    fixed_disks = $disks
    boundary = 'QGA noninteractive black-box command/path inventory only; no secrets, REA, IL inspection, decompilation, or scheduled tasks.'
} | ConvertTo-Json -Depth 7

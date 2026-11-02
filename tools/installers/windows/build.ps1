<#
.SYNOPSIS
Builds the unsigned Paint.Rust Windows x86_64 development-preview installer.

.DESCRIPTION
Validates the product version and the x86_64 PE application binary, locates NSIS 3
(makensis), compiles tools/installers/windows/paint-rust.nsi, verifies the setup
executable, and writes a SHA-256 sidecar. The output is not code-signed.

.PARAMETER Version
Semantic product version, for example 0.1.1 or 0.1.1-preview.1. Build metadata
and leading zero numeric components are not accepted.

.PARAMETER BinaryPath
Path to the release paint-ui.exe. It must be an x86_64 PE executable.

.PARAMETER OutputDir
Directory that receives Paint.Rust-<Version>-windows-x86_64-setup.exe and its
.sha256 sidecar. The directory is created when missing.

.PARAMETER MakensisPath
Optional explicit path to makensis.exe. Otherwise PATH and the standard
Program Files (x86)\NSIS location are searched.

.PARAMETER PlanOnly
Validate inputs and print the resolved build plan as JSON. Does not require
makensis and does not create output files.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Version,

    [Parameter(Mandatory = $true)]
    [string] $BinaryPath,

    [Parameter(Mandatory = $true)]
    [string] $OutputDir,

    [string] $MakensisPath = '',

    [switch] $PlanOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$scriptRoot = $PSScriptRoot
$optionPrefix = '/'
if ([System.Environment]::OSVersion.Platform -ne [System.PlatformID]::Win32NT) {
    $optionPrefix = '-'
}
$nsiScriptPath = Join-Path $scriptRoot 'paint-rust.nsi'
$installTextPath = Join-Path $scriptRoot 'INSTALL-WINDOWS.txt'
$repoRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $scriptRoot))
$licensePath = Join-Path $repoRoot 'LICENSE'

function Fail {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Message
    )

    throw "Paint.Rust Windows installer build failed: $Message"
}

function Resolve-ExistingFile {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,

        [Parameter(Mandatory = $true)]
        [string] $Label
    )

    if ([string]::IsNullOrWhiteSpace($Path)) {
        Fail "$Label must not be empty."
    }

    try {
        $item = Get-Item -LiteralPath $Path -Force -ErrorAction Stop
    }
    catch {
        Fail "$Label was not found: $Path"
    }

    if ($item.PSIsContainer) {
        Fail "$Label must be a file: $Path"
    }

    return $item.FullName
}

function Get-InstallerVersion {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Raw
    )

    if ([string]::IsNullOrWhiteSpace($Raw)) {
        Fail 'Version must not be empty.'
    }
    if ($Raw -cne $Raw.Trim()) {
        Fail "Version '$Raw' must not contain leading or trailing whitespace."
    }
    if ($Raw.Length -gt 64) {
        Fail 'Version must be 64 characters or fewer.'
    }

    $versionPattern = '^(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)\.(?:0|[1-9][0-9]*)(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?$'
    if (-not [regex]::IsMatch($Raw, $versionPattern)) {
        Fail "Version '$Raw' must be a semantic version such as 0.1.1 or 0.1.1-preview.1; build metadata and leading-zero numeric components are not accepted."
    }

    $core = ($Raw -split '-', 2)[0]
    $parts = $core.Split('.')
    $numbers = @()
    foreach ($part in $parts) {
        $value = 0
        if (-not [int]::TryParse($part, [ref] $value)) {
            Fail "Version '$Raw' has a numeric component that is too large."
        }
        if ($value -lt 0 -or $value -gt 65535) {
            Fail "Version '$Raw' numeric components must be between 0 and 65535."
        }
        $numbers += $value
    }

    $major = $numbers[0]
    $minor = $numbers[1]
    $patch = $numbers[2]
    return [pscustomobject]@{
        Text = $Raw
        Numeric = ('{0}.{1}.{2}.0' -f $major, $minor, $patch)
    }
}

function Resolve-OutputDirectory {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    if ([string]::IsNullOrWhiteSpace($Path)) {
        Fail 'OutputDir must not be empty.'
    }

    $fullPath = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
    if ([string]::IsNullOrWhiteSpace($fullPath)) {
        Fail "OutputDir could not be resolved: $Path"
    }
    if (Test-Path -LiteralPath $fullPath -PathType Leaf) {
        Fail "OutputDir is an existing file: $fullPath"
    }

    return $fullPath
}

function Assert-X64Pe {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $stream = $null
    $reader = $null
    try {
        $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
        if ($stream.Length -lt 64) {
            Fail "Binary is too small to be a PE executable: $Path"
        }

        $reader = New-Object -TypeName System.IO.BinaryReader -ArgumentList $stream
        $mz = $reader.ReadBytes(2)
        if ($mz.Length -ne 2 -or $mz[0] -ne 0x4D -or $mz[1] -ne 0x5A) {
            Fail "Binary is not a PE executable (missing MZ header): $Path"
        }

        $stream.Position = 0x3C
        $peOffset = $reader.ReadInt32()
        if ($peOffset -lt 0 -or $peOffset -gt ($stream.Length - 6)) {
            Fail "Binary has an invalid PE header offset: $Path"
        }

        $stream.Position = $peOffset
        $signature = $reader.ReadBytes(4)
        if ($signature.Length -ne 4 -or $signature[0] -ne 0x50 -or $signature[1] -ne 0x45 -or $signature[2] -ne 0x00 -or $signature[3] -ne 0x00) {
            Fail "Binary is not a PE executable (missing PE signature): $Path"
        }

        $machine = $reader.ReadUInt16()
        if ($machine -ne 0x8664) {
            Fail ("Binary is not an x86_64 PE executable (machine 0x{0:X4}); expected 0x8664: {1}" -f $machine, $Path)
        }
    }
    finally {
        if ($null -ne $reader) {
            $reader.Dispose()
        }
        elseif ($null -ne $stream) {
            $stream.Dispose()
        }
    }
}

function Assert-MzExecutable {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $stream = $null
    $reader = $null
    try {
        $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
        $reader = New-Object -TypeName System.IO.BinaryReader -ArgumentList $stream
        $magic = $reader.ReadBytes(2)
        if ($magic.Length -ne 2 -or $magic[0] -ne 0x4D -or $magic[1] -ne 0x5A) {
            Fail "Setup executable does not have an MZ header: $Path"
        }
    }
    finally {
        if ($null -ne $reader) {
            $reader.Dispose()
        }
        elseif ($null -ne $stream) {
            $stream.Dispose()
        }
    }
}

function Get-Sha256Hex {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path
    )

    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    $stream = $null
    try {
        $stream = [System.IO.File]::Open($Path, [System.IO.FileMode]::Open, [System.IO.FileAccess]::Read, [System.IO.FileShare]::Read)
        $hashBytes = $algorithm.ComputeHash($stream)
    }
    finally {
        if ($null -ne $stream) {
            $stream.Dispose()
        }
        $algorithm.Dispose()
    }

    $hex = [System.BitConverter]::ToString($hashBytes)
    return $hex.Replace('-', '').ToLowerInvariant()
}

function Resolve-Makensis {
    param(
        [string] $ExplicitPath
    )

    if (-not [string]::IsNullOrWhiteSpace($ExplicitPath)) {
        return (Resolve-ExistingFile -Path $ExplicitPath -Label 'MakensisPath')
    }

    foreach ($name in @('makensis.exe', 'makensis')) {
        $command = Get-Command -Name $name -CommandType Application -ErrorAction SilentlyContinue
        if ($null -ne $command) {
            $commandPath = $command.Source
            if ([string]::IsNullOrWhiteSpace($commandPath)) {
                $commandPath = $command.Definition
            }
            if (-not [string]::IsNullOrWhiteSpace($commandPath)) {
                return $commandPath
            }
        }
    }

    $candidates = @()
    $programFilesX86 = [Environment]::GetEnvironmentVariable('ProgramFiles(x86)')
    $programFiles = [Environment]::GetEnvironmentVariable('ProgramFiles')
    if (-not [string]::IsNullOrWhiteSpace($programFilesX86)) {
        $candidates += (Join-Path $programFilesX86 'NSIS\makensis.exe')
    }
    if (-not [string]::IsNullOrWhiteSpace($programFiles)) {
        $candidates += (Join-Path $programFiles 'NSIS\makensis.exe')
    }

    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath $candidate -PathType Leaf) {
            return (Resolve-Path -LiteralPath $candidate).ProviderPath
        }
    }

    return $null
}

$nsiScriptPath = Resolve-ExistingFile -Path $nsiScriptPath -Label 'NSIS script'
$installTextPath = Resolve-ExistingFile -Path $installTextPath -Label 'INSTALL-WINDOWS.txt'
$licensePath = Resolve-ExistingFile -Path $licensePath -Label 'Repository LICENSE'
$versionInfo = Get-InstallerVersion -Raw $Version

$binaryFull = Resolve-ExistingFile -Path $BinaryPath -Label 'BinaryPath'
if ([System.IO.Path]::GetFileName($binaryFull) -ine 'paint-ui.exe') {
    Fail "BinaryPath must point to paint-ui.exe, not '$([System.IO.Path]::GetFileName($binaryFull))'."
}
Assert-X64Pe -Path $binaryFull

$outputDirFull = Resolve-OutputDirectory -Path $OutputDir
$setupName = 'Paint.Rust-{0}-windows-x86_64-setup.exe' -f $versionInfo.Text
$setupPath = Join-Path $outputDirFull $setupName
$sidecarPath = '{0}.sha256' -f $setupPath
$makensis = Resolve-Makensis -ExplicitPath $MakensisPath

if ($PlanOnly) {
    $plan = [ordered]@{
        schema = 'paint-rust/windows-installer-plan@1'
        version = $versionInfo.Text
        version_numeric = $versionInfo.Numeric
        binary = $binaryFull
        binary_sha256 = (Get-Sha256Hex -Path $binaryFull)
        output_dir = $outputDirFull
        setup = $setupPath
        sidecar = $sidecarPath
        installer_script = $nsiScriptPath
        install_text = $installTextPath
        license = $licensePath
        makensis = $makensis
        makensis_found = ($null -ne $makensis)
        plan_only = $true
        unsigned = $true
    }
    $plan | ConvertTo-Json -Depth 4 -Compress
    return
}

if ($null -eq $makensis) {
    Fail 'makensis.exe was not found on PATH or in the standard NSIS installation directory. Install NSIS 3.x or pass -MakensisPath.'
}

if (-not (Test-Path -LiteralPath $outputDirFull -PathType Container)) {
    New-Item -ItemType Directory -Path $outputDirFull -Force | Out-Null
}

foreach ($stalePath in @($setupPath, $sidecarPath)) {
    if (Test-Path -LiteralPath $stalePath) {
        Remove-Item -LiteralPath $stalePath -Force -ErrorAction Stop
    }
}

$makensisArguments = @(
    ($optionPrefix + 'V2')
    ($optionPrefix + 'DVERSION=' + $versionInfo.Text)
    ($optionPrefix + 'DVI_VERSION=' + $versionInfo.Numeric)
    ($optionPrefix + 'DBINARY=' + $binaryFull)
    ($optionPrefix + 'DOUTFILE=' + $setupPath)
    ($optionPrefix + 'DINSTALL_TEXT=' + $installTextPath)
    ($optionPrefix + 'DLICENSE_FILE=' + $licensePath)
    $nsiScriptPath
)

& $makensis @makensisArguments
$makensisExit = $LASTEXITCODE
if ($makensisExit -ne 0) {
    Fail "makensis exited with code $makensisExit."
}

if (-not (Test-Path -LiteralPath $setupPath -PathType Leaf)) {
    Fail "makensis reported success but did not create the setup executable: $setupPath"
}
$setupItem = Get-Item -LiteralPath $setupPath -Force -ErrorAction Stop
if ($setupItem.Length -le 0) {
    Fail "Setup executable is empty: $setupPath"
}
Assert-MzExecutable -Path $setupPath

$setupHash = Get-Sha256Hex -Path $setupPath
if ($setupHash -notmatch '^[0-9a-f]{64}$') {
    Fail "Setup executable SHA-256 is not a lowercase 64-character hex digest: $setupHash"
}

$sidecarContent = '{0}  {1}{2}' -f $setupHash, $setupName, "`n"
$utf8NoBom = New-Object -TypeName System.Text.UTF8Encoding -ArgumentList $false
[System.IO.File]::WriteAllText($sidecarPath, $sidecarContent, $utf8NoBom)
$sidecarReadBack = [System.IO.File]::ReadAllText($sidecarPath, [System.Text.Encoding]::UTF8)
if ($sidecarReadBack -cne $sidecarContent) {
    Fail "SHA-256 sidecar did not round-trip exactly: $sidecarPath"
}

Write-Output ('setup: {0}' -f $setupPath)
Write-Output ('sha256: {0}' -f $setupHash)
Write-Output ('sidecar: {0}' -f $sidecarPath)

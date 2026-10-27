$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

function Convert-RegistryValue($Value) {
    if ($Value -is [byte[]]) {
        return [pscustomobject]@{ type = "byte[]"; length = $Value.Length; base64 = [Convert]::ToBase64String($Value) }
    }
    return $Value
}

function Get-RegistryValues([string]$Root, [string[]]$Names) {
    $result = [ordered]@{}
    foreach ($name in $Names) {
        try {
            $value = (Get-ItemProperty -LiteralPath $Root -Name $name -ErrorAction Stop).$name
            $result[$name] = Convert-RegistryValue $value
        } catch {
            $result[$name] = $null
        }
    }
    return [pscustomobject]$result
}

function Get-RegistryBag([string]$Root) {
    $result = [ordered]@{}
    try {
        $properties = Get-ItemProperty -LiteralPath $Root -ErrorAction Stop
        foreach ($property in $properties.PSObject.Properties) {
            if ($property.Name -notlike "PS*") { $result[$property.Name] = Convert-RegistryValue $property.Value }
        }
    } catch {}
    return [pscustomobject]$result
}

function Get-ProfileAssociations([string]$Sid) {
    $root = "Registry::HKEY_USERS\$Sid\Software\Microsoft\Windows NT\CurrentVersion\ICM\ProfileAssociations\Display"
    $items = @()
    try {
        foreach ($key in Get-ChildItem -LiteralPath $root -ErrorAction Stop | Select-Object -First 64) {
            $bag = Get-RegistryBag $key.PSPath
            $items += [pscustomobject]@{ monitor_key = $key.PSChildName; values = $bag }
            foreach ($child in Get-ChildItem -LiteralPath $key.PSPath -ErrorAction SilentlyContinue | Select-Object -First 16) {
                $items += [pscustomobject]@{ monitor_key = ($key.PSChildName + "\" + $child.PSChildName); values = (Get-RegistryBag $child.PSPath) }
            }
        }
    } catch {}
    return ,$items
}

$explorer = Get-Process explorer -IncludeUserName -ErrorAction Stop | Where-Object { $_.SessionId -gt 0 } | Select-Object -First 1
if ($null -eq $explorer) { throw "No interactive explorer process found" }
$sid = (New-Object System.Security.Principal.NTAccount($explorer.UserName)).Translate([System.Security.Principal.SecurityIdentifier]).Value
$userRoot = "Registry::HKEY_USERS\$sid"

$os = Get-CimInstance Win32_OperatingSystem
$currentVersion = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion"
$computerSystem = Get-CimInstance Win32_ComputerSystem
$processor = Get-CimInstance Win32_Processor | Select-Object -First 1
$osEvidence = [pscustomobject]@{
    caption = $os.Caption
    version = $os.Version
    build_number = $os.BuildNumber
    build_lab_ex = $currentVersion.BuildLabEx
    display_version = $currentVersion.DisplayVersion
    edition_id = $currentVersion.EditionID
    installation_type = $currentVersion.InstallationType
    ubr = $currentVersion.UBR
    native_system_type = $computerSystem.SystemType
    native_os_architecture = $os.OSArchitecture
    processor_architecture_code = $processor.Architecture
    processor_address_width = $processor.AddressWidth
    qga_process_architecture = $env:PROCESSOR_ARCHITECTURE
    current_major_minor_build = $currentVersion.CurrentMajorVersionNumber.ToString() + "." + $currentVersion.CurrentMinorVersionNumber.ToString() + "." + $currentVersion.CurrentBuildNumber
}

$paintPath = "C:\Program Files\paint.net\paintdotnet.exe"
$paintFile = Get-Item -LiteralPath $paintPath
$paintSignature = Get-AuthenticodeSignature -LiteralPath $paintPath
$paintProcess = Get-Process paintdotnet -ErrorAction Stop | Select-Object -First 1
$paintEvidence = [pscustomobject]@{
    path = $paintFile.FullName
    product_version = $paintFile.VersionInfo.ProductVersion
    file_version = $paintFile.VersionInfo.FileVersion
    length = $paintFile.Length
    sha256 = (Get-FileHash -LiteralPath $paintPath -Algorithm SHA256).Hash.ToLowerInvariant()
    signature_status = $paintSignature.Status.ToString()
    signer_subject = $paintSignature.SignerCertificate.Subject
    process_id = $paintProcess.Id
    process_session_id = $paintProcess.SessionId
    process_start_time = $paintProcess.StartTime.ToString("o")
}

$displayDevices = @(Get-CimInstance Win32_VideoController | ForEach-Object {
    [pscustomobject]@{
        name = $_.Name
        driver_version = $_.DriverVersion
        driver_date = if ($null -eq $_.DriverDate) { $null } else { $_.DriverDate.ToString("o") }
        video_mode_description = $_.VideoModeDescription
        current_horizontal_resolution = $_.CurrentHorizontalResolution
        current_vertical_resolution = $_.CurrentVerticalResolution
        current_bits_per_pixel = $_.CurrentBitsPerPixel
        current_refresh_rate = $_.CurrentRefreshRate
        min_refresh_rate = $_.MinRefreshRate
        max_refresh_rate = $_.MaxRefreshRate
        video_processor = $_.VideoProcessor
        adapter_dac_type = $_.AdapterDACType
        status = $_.Status
    }
})
$desktopMonitors = @(Get-CimInstance Win32_DesktopMonitor -ErrorAction SilentlyContinue | ForEach-Object {
    [pscustomobject]@{
        name = $_.Name
        monitor_type = $_.MonitorType
        screen_width = $_.ScreenWidth
        screen_height = $_.ScreenHeight
        bandwidth = $_.Bandwidth
        pixels_per_x_logical_inch = $_.PixelsPerXLogicalInch
        pixels_per_y_logical_inch = $_.PixelsPerYLogicalInch
    }
})

$desktop = Get-RegistryValues "$userRoot\Control Panel\Desktop" @(
    "LogPixels", "AppliedDPI", "Win8DpiScaling", "DesktopDPIOverride", "FontSmoothing",
    "FontSmoothingType", "FontSmoothingGamma", "FontSmoothingOrientation"
)
$windowMetrics = Get-RegistryValues "$userRoot\Control Panel\Desktop\WindowMetrics" @(
    "BorderWidth", "CaptionWidth", "CaptionHeight", "MenuWidth", "MenuHeight", "ScrollWidth", "ScrollHeight",
    "IconSpacing", "IconVerticalSpacing", "MinWidth", "CaptionFont", "IconFont", "MenuFont", "MessageFont",
    "SmCaptionFont", "StatusFont"
)
$personalize = Get-RegistryValues "$userRoot\Software\Microsoft\Windows\CurrentVersion\Themes\Personalize" @(
    "AppsUseLightTheme", "SystemUsesLightTheme", "EnableTransparency", "ColorPrevalence"
)
$dwm = Get-RegistryValues "$userRoot\Software\Microsoft\Windows\DWM" @(
    "AccentColor", "ColorizationColor", "ColorizationAfterglow", "ColorPrevalence", "EnableWindowColorization"
)
$colors = Get-RegistryValues "$userRoot\Control Panel\Colors" @(
    "ActiveBorder", "ActiveTitle", "AppWorkspace", "Background", "ButtonAlternateFace", "ButtonDkShadow",
    "ButtonFace", "ButtonHilight", "ButtonLight", "ButtonShadow", "ButtonText", "GrayText", "Hilight",
    "HilightText", "HotTrackingColor", "InactiveBorder", "InactiveTitle", "InactiveTitleText", "InfoText",
    "InfoWindow", "Menu", "MenuHilight", "MenuBar", "MenuText", "Scrollbar", "TitleText", "Window",
    "WindowFrame", "WindowText"
)
$accessibility = Get-RegistryValues "$userRoot\Software\Microsoft\Accessibility" @("TextScaleFactor")
$highContrast = Get-RegistryValues "$userRoot\Control Panel\Accessibility\HighContrast" @("Flags", "High Contrast Scheme")
$international = Get-RegistryValues "$userRoot\Control Panel\International" @(
    "LocaleName", "sLanguage", "sCountry", "sDecimal", "sThousand", "sShortDate", "sLongDate", "sShortTime",
    "sTimeFormat", "iFirstDayOfWeek"
)
$keyboard = Get-RegistryBag "$userRoot\Keyboard Layout\Preload"
$theme = Get-RegistryValues "$userRoot\Software\Microsoft\Windows\CurrentVersion\Themes" @("CurrentTheme", "InstallTheme")
$greFonts = Get-RegistryValues "HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion\GRE_Initialize" @(
    "GUIFont.Facename", "GUIFont.Height", "GUIFont.Weight", "GUIFont.Italic", "GUIFont.CharSet"
)

$fontFiles = @()
$fontRoot = Join-Path $env:SystemRoot "Fonts"
foreach ($name in @("segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf", "arial.ttf", "tahoma.ttf", "micross.ttf")) {
    $path = Join-Path $fontRoot $name
    if (Test-Path -LiteralPath $path) {
        $item = Get-Item -LiteralPath $path
        $fontFiles += [pscustomobject]@{ name = $name; length = $item.Length; sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant() }
    } else {
        $fontFiles += [pscustomobject]@{ name = $name; length = $null; sha256 = $null }
    }
}
$colorFiles = @()
$colorRoot = Join-Path $env:SystemRoot "System32\spool\drivers\color"
foreach ($item in Get-ChildItem -LiteralPath $colorRoot -File -ErrorAction SilentlyContinue | Sort-Object Name | Select-Object -First 128) {
    $colorFiles += [pscustomobject]@{ name = $item.Name; length = $item.Length; sha256 = (Get-FileHash -LiteralPath $item.FullName -Algorithm SHA256).Hash.ToLowerInvariant() }
}

$hotfixes = @(Get-HotFix -ErrorAction SilentlyContinue | Sort-Object HotFixID | ForEach-Object {
    [pscustomobject]@{ hotfix_id = $_.HotFixID; description = $_.Description; installed_on = if ($null -eq $_.InstalledOn) { $null } else { $_.InstalledOn.ToString("yyyy-MM-dd") } }
})
$timeZone = Get-TimeZone
$environment = [pscustomobject]@{
    schema_version = 1
    collection_context = [pscustomobject]@{
        collector = "QGA noninteractive Windows PowerShell; interactive user registry read through HKEY_USERS; no scheduled task"
        interactive_session_id = $explorer.SessionId
        collected_at = (Get-Date).ToString("o")
        time_zone_id = $timeZone.Id
        time_zone_display_name = $timeZone.DisplayName
    }
    os = $osEvidence
    paintnet = $paintEvidence
    display = [pscustomobject]@{ video_controllers = $displayDevices; desktop_monitors = $desktopMonitors }
    dpi_and_text = [pscustomobject]@{ desktop = $desktop; window_metrics = $windowMetrics; accessibility = $accessibility; high_contrast = $highContrast }
    theme_and_color = [pscustomobject]@{ personalize = $personalize; dwm = $dwm; system_colors = $colors; theme = $theme; icm_display_associations = (Get-ProfileAssociations $sid); installed_color_profiles = $colorFiles }
    fonts = [pscustomobject]@{ gre_initialize = $greFonts; selected_font_files = $fontFiles }
    locale_and_input = [pscustomobject]@{ international = $international; keyboard_preload = $keyboard }
    installed_hotfixes = $hotfixes
    unpinned_fields = @(
        "Windows installation ISO identity/hash, activation/license state, update catalog state, and pending reboot state",
        "Exact effective per-monitor DPI and DPI awareness context observed by Paint.NET; registry and CIM values are not an interactive DPI API query",
        "HDR/advanced-color state, pixel format beyond CIM's report, VRR, and physical refresh rate; CIM refresh-rate values may be virtual adapters' placeholders",
        "ICC profile and color-conversion policy actually selected by Paint.NET; registry associations and installed profiles do not prove application selection",
        "Unobserved Paint.NET settings, recent files, effect defaults, and default text-tool font",
        "Exhaustive fonts, fallback, caches, and shaping; only selected font files are hashed",
        "Host brightness, True Tone, Night Shift, ICC profile, and other host color state",
        "Wall clock and future updates; dynamic resolution is bounded only by the harness window-layout check"
    )
    privacy = "No keys, license IDs, user/SID, recent files, clipboard, or hardware serials collected."
}
$environment | ConvertTo-Json -Depth 10 -Compress

on run argv
    if (count of argv) is not 1 then error "Usage: utm_inventory.applescript <vm>" number 64
    set vmName to item 1 of argv
    tell application "UTM"
        set utmVersion to UTM version
        set vm to virtual machine vmName
        set vmConfiguration to configuration of vm
        set displayConfigurations to displays of vmConfiguration
        if (count of displayConfigurations) is not 1 then error "Expected exactly one UTM display" number 65
        set displayConfiguration to item 1 of displayConfigurations
        set inventoryValues to {utmVersion, id of vm, backend of vm, status of vm, architecture of vmConfiguration, memory of vmConfiguration, cpu cores of vmConfiguration, hypervisor of vmConfiguration, uefi of vmConfiguration, hardware of displayConfiguration, dynamic resolution of displayConfiguration, native resolution of displayConfiguration, upscaling filter of displayConfiguration, downscaling filter of displayConfiguration}
    end tell
    set oldDelimiters to AppleScript's text item delimiters
    set AppleScript's text item delimiters to linefeed
    set rendered to inventoryValues as text
    set AppleScript's text item delimiters to oldDelimiters
    return rendered
end run

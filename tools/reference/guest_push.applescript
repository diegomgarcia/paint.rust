on run argv
    if (count of argv) is not 3 then error "Usage: guest_push.applescript <vm> <guest-path> <host-path>" number 64
    set vmName to item 1 of argv
    set guestPath to item 2 of argv
    set hostPath to item 3 of argv
    set hostFile to POSIX file hostPath
    tell application "UTM"
        set vm to virtual machine vmName
        if status of vm is not started then error "UTM VM is not started" number 69
        set guestFile to open file vm at guestPath for writing
        try
            push guestFile from hostFile closing true
        on error errorMessage number errorNumber
            try
                close guestFile
            end try
            error errorMessage number errorNumber
        end try
    end tell
    return "pushed"
end run

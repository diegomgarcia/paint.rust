on run argv
    if (count of argv) is less than 3 then error "Usage: guest_exec.applescript <vm> <timeout-seconds> <executable> [arguments ...]" number 64
    set vmName to item 1 of argv
    set timeoutSeconds to (item 2 of argv) as integer
    if timeoutSeconds is less than 1 or timeoutSeconds is greater than 300 then error "Timeout must be 1..300 seconds" number 64
    set executablePath to item 3 of argv
    set guestArguments to {}
    repeat with argumentIndex from 4 to count of argv
        set end of guestArguments to item argumentIndex of argv
    end repeat
    tell application "UTM"
        set vm to virtual machine vmName
        if status of vm is not started then error "UTM VM is not started" number 69
        set guestProcess to execute vm at executablePath with arguments guestArguments output capturing true
    end tell
    repeat (timeoutSeconds * 2) times
        tell application "UTM"
            set processResult to get result guestProcess
            set isExited to exited of processResult
        end tell
        if isExited then
            tell application "UTM"
                set guestExitCode to exit code of processResult
                if guestExitCode is not 0 then
                    set guestError to error text of processResult
                    error ("Guest process exit code " & guestExitCode & ": " & guestError) number 65
                end if
                return output data of processResult
            end tell
        end if
        delay 0.5
    end repeat
    error ("Guest process did not exit within " & timeoutSeconds & " seconds") number 124
end run

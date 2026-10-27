#!/usr/bin/env bash
# Launch a real Paint.R binary, require its native window, and close it cleanly.
set -u

if [[ $# -ne 1 ]]; then
  printf 'usage: %s <path-to-paint-ui>\n' "$0" >&2
  exit 64
fi
binary=$1
if [[ ! -x $binary ]]; then
  printf 'not an executable Paint.R binary: %s\n' "$binary" >&2
  exit 66
fi
if [[ $(uname -s) != Darwin ]]; then
  printf 'this native-window smoke requires macOS\n' >&2
  exit 69
fi

log=$(mktemp "${TMPDIR:-/tmp}/paint-r-native-window.XXXXXX") || exit 1
"$binary" >"$log" 2>&1 &
pid=$!
succeeded=false
cleanup() {
  if kill -0 "$pid" 2>/dev/null; then
    kill "$pid" 2>/dev/null
    wait "$pid" 2>/dev/null
  fi
  if [[ $succeeded == true && -f $log && $log == "${TMPDIR:-/tmp}"/paint-r-native-window.* ]]; then
    rm -f -- "$log"
  elif [[ -f $log ]]; then
    printf 'native-window log: %s\n' "$log" >&2
  fi
}
trap cleanup EXIT INT TERM

window=''
last_state=''
for _attempt in $(seq 1 45); do
  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid"
    printf 'native process exited before a window appeared\n' >&2
    exit 1
  fi
  state=$(osascript - "$pid" <<'APPLESCRIPT'
on run argv
  set targetPid to (item 1 of argv) as integer
  tell application "System Events"
    try
      set targetProcess to first process whose unix id is targetPid
      if (count of windows of targetProcess) > 0 then
        return name of first window of targetProcess
      end if
      return "NO_WINDOW"
    on error errorMessage
      return "ERROR: " & errorMessage
    end try
  end tell
end run
APPLESCRIPT
)
  last_state=$state
  case "$state" in
    NO_WINDOW|ERROR:*) sleep 1 ;;
    *) window=$state; break ;;
  esac
done
if [[ -z $window ]]; then
  printf 'no native window appeared; last state=%s\n' "$last_state" >&2
  exit 1
fi
printf 'native window observed: pid=%s title=%s\n' "$pid" "$window"

if ! osascript - "$pid" <<'APPLESCRIPT'
on run argv
  set targetPid to (item 1 of argv) as integer
  tell application "System Events"
    set targetProcess to first process whose unix id is targetPid
    set frontmost of targetProcess to true
    keystroke "q" using command down
  end tell
end run
APPLESCRIPT
then
  printf 'failed to send Command-Q to pid=%s\n' "$pid" >&2
  exit 1
fi

for _attempt in $(seq 1 30); do
  if ! kill -0 "$pid" 2>/dev/null; then
    wait "$pid"
    status=$?
    if [[ $status -eq 0 ]]; then
      succeeded=true
      printf 'native graceful quit status=0\n'
    else
      printf 'native graceful quit status=%s\n' "$status" >&2
    fi
    exit "$status"
  fi
  sleep 1
done
printf 'native app did not quit within 30 seconds\n' >&2
exit 1

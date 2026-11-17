#!/bin/bash
# Takes the raw Google Play screenshots on a connected Android phone, with
# the demo build (made-up mail: see android/app/src/main/kotlin/com/ferrowl/
# app/demo/Demo.kt). The demo build is its own app, com.ferrowl.app.demo,
# beside the real one (com.ferrowl.app), which this script never touches.
#
#   (cd android && ANDROID_SERIAL=<device> ./gradlew :app:installDemo)
#   store/tools/capture_android.sh <device> [locale ...] [-- scene ...]
#
# Writes store/android/<locale>/raw/<n>_<scene>.png. The demo app shows
# over the lock screen (the phone stays locked) without the system bars; a
# capture with something else on screen (another app, the notification
# shade, a notification popping up) is taken again.
set -euo pipefail
device="$1"; shift
root="$(cd "$(dirname "$0")/../.." && pwd)"
app=com.ferrowl.app.demo
adb=(adb -s "$device")

locales=()
while [[ $# -gt 0 && "$1" != "--" ]]; do locales+=("$1"); shift; done
[[ "${1:-}" == "--" ]] && shift
scenes=("$@")
[[ ${#locales[@]} -eq 0 ]] && locales=(en-US it-IT es-ES fr-FR de-DE pt-BR ja-JP ko-KR zh-CN ar)
[[ ${#scenes[@]} -eq 0 ]] && scenes=(inbox message undo accounts invite calendar)

number() {
    case "$1" in
        inbox) echo 01 ;; message) echo 02 ;; undo) echo 03 ;; accounts) echo 04 ;; invite) echo 05 ;;
        calendar) echo 06 ;; pgp) echo 07 ;; *) echo 09 ;;
    esac
}

"${adb[@]}" shell pm path "$app" >/dev/null || { echo "$app is not installed: (cd android && ./gradlew :app:installDemo)" >&2; exit 1; }

# The demo app's own language setting, put back at the end.
before="$("${adb[@]}" shell cmd locale get-app-locales "$app" | sed -n 's/.*are \[\(.*\)\].*/\1/p')"
demo_allowed="$("${adb[@]}" shell settings get global sysui_demo_allowed)"

demo() { "${adb[@]}" shell am broadcast -a com.android.systemui.demo -e command "$@" >/dev/null; }

finish() {
    # Off the screen (it shows over the lock screen), and settings as they were.
    "${adb[@]}" shell am force-stop "$app" || true
    "${adb[@]}" shell cmd locale set-app-locales "$app" --locales "$before" || true
    demo exit || true
    [[ "$demo_allowed" == "1" ]] || "${adb[@]}" shell settings put global sysui_demo_allowed "${demo_allowed/null/0}" || true
}
trap finish EXIT

# A clean status bar where SystemUI allows it (Samsung ignores it; the demo
# app hides the system bars anyway).
"${adb[@]}" shell settings put global sysui_demo_allowed 1
demo enter
demo clock -e hhmm 1200
demo battery -e level 100 -e plugged false
demo network -e wifi show -e level 4
demo network -e mobile show -e datatype none -e level 4
demo notifications -e visible false

# Whether the capture shows the demo app alone: it has the focus (not the
# shade, another app, or the lock screen after a crash), no notification
# pops up, and the strip at the top (the camera's, empty in the app) has no
# banner over it.
clean() {
    local file="$1"
    "${adb[@]}" shell dumpsys window | grep -q "mCurrentFocus=.*$app/" || return 1
    "${adb[@]}" shell dumpsys activity service com.android.systemui/.SystemUIService \
        | awk '/HeadsUpManager state:/{f=1} f&&/mTouchableRegion/{print; exit}' | grep -q 'SkRegion()' || return 1
    python3 - "$file" <<'EOF'
import sys
from PIL import Image
im = Image.open(sys.argv[1]).convert("RGB")
w = im.width
# The columns of the top strip are each of one color (the app, or the
# drawer and its shade; a few at the drawer's rounded corner are not); a
# notification would bring edges and text across much of it.
columns = range(0, w, 4)
mixed = 0
for x in columns:
    column = [im.getpixel((x, y)) for y in range(4, 60, 2)]
    if max(max(abs(a - b) for a, b in zip(p, column[0])) for p in column) > 12:
        mixed += 1
sys.exit(1 if mixed > len(columns) // 10 else 0)
EOF
}

for locale in "${locales[@]}"; do
    # Play's locale is the app's (language and region: dates as there).
    "${adb[@]}" shell cmd locale set-app-locales "$app" --locales "$locale"
    out="$root/store/android/$locale/raw"
    mkdir -p "$out"
    for scene in "${scenes[@]}"; do
        file="$out/$(number "$scene")_$scene.png"
        rm -f "$file"
        for try in 1 2 3 4 5 6; do
            "${adb[@]}" shell input keyevent KEYCODE_WAKEUP
            "${adb[@]}" shell am start -S -W -n "$app/com.ferrowl.app.MainActivity" --es ferrowl_demo "$scene" >/dev/null
            sleep 6
            "${adb[@]}" exec-out screencap -p >"$file.tmp"
            if clean "$file.tmp"; then
                mv "$file.tmp" "$file"
                break
            fi
            echo "$locale $scene: something else on screen, again"
            rm -f "$file.tmp"
            sleep 4
        done
        [[ -f "$file" ]] && echo "$locale $scene" || echo "$locale $scene: FAILED"
    done
done

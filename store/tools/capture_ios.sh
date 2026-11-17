#!/bin/bash
# Takes the raw App Store screenshots on a connected iPhone, with the app's
# demo mode (a Debug build: see ios/Ferrowl/Data/Demo.swift). Never a simulator.
#
#   store/tools/capture_ios.sh <UDID> [locale ...] [-- scene ...]
#
# Writes store/ios/<locale>/screenshots/raw/<n>_<scene>.png, then relaunches
# the app without demo mode.
set -euo pipefail
device="$1"; shift
root="$(cd "$(dirname "$0")/../.." && pwd)"

locales=()
while [[ $# -gt 0 && "$1" != "--" ]]; do locales+=("$1"); shift; done
[[ "${1:-}" == "--" ]] && shift
scenes=("$@")
[[ ${#locales[@]} -eq 0 ]] && locales=(en-US it es-ES fr-FR de-DE pt-BR ja ko zh-Hans ar-SA)
[[ ${#scenes[@]} -eq 0 ]] && scenes=(inbox message undo accounts pgp calendar)

# Store locale -> app language and region.
lang() {
    case "$1" in
        en-US) echo "en en_US" ;; it) echo "it it_IT" ;; es-ES) echo "es es_ES" ;;
        fr-FR) echo "fr fr_FR" ;; de-DE) echo "de de_DE" ;; pt-BR) echo "pt-BR pt_BR" ;;
        ja) echo "ja ja_JP" ;; ko) echo "ko ko_KR" ;; zh-Hans) echo "zh-Hans zh_CN" ;; ar-SA) echo "ar ar_SA" ;;
    esac
}

number() {
    case "$1" in
        inbox) echo 01 ;; message) echo 02 ;; undo) echo 03 ;; accounts) echo 04 ;; pgp) echo 05 ;;
        calendar) echo 06 ;; *) echo 09 ;;
    esac
}

# Notifications of other apps must not end up in the images: a capture
# with a banner at the top is taken again.
banner="$(mktemp -d)/banner"
swiftc -O "$root/store/tools/banner.swift" -o "$banner"

for locale in "${locales[@]}"; do
    read -r language region <<<"$(lang "$locale")"
    out="$root/store/ios/$locale/screenshots/raw"
    mkdir -p "$out"
    for scene in "${scenes[@]}"; do
        # A launch over Wi-Fi fails now and then: tried again.
        for try in 1 2 3; do
            xcrun devicectl device process launch --quiet --terminate-existing --device "$device" \
                --environment-variables "{\"FERROWL_DEMO\":\"$scene\"}" com.ferrowl.app \
                -- -AppleLanguages "($language)" -AppleLocale "$region" && break
            sleep 3
        done
        sleep 6
        file="$out/$(number "$scene")_$scene.png"
        for try in 1 2 3 4 5 6; do
            xcrun devicectl device capture screenshot --quiet --device "$device" --destination "$file"
            "$banner" "$file" && break
            echo "$locale $scene: notification banner, again"
            rm -f "$file"
            sleep 5
        done
        [[ -f "$file" ]] && echo "$locale $scene" || echo "$locale $scene: FAILED"
    done
done

# Back to the user's own mail.
xcrun devicectl device process launch --quiet --terminate-existing --device "$device" com.ferrowl.app

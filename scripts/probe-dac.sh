#!/usr/bin/env bash
# Collect read-only ALSA/USB evidence for every USB audio device.
# Never opens a PCM device and never plays audio.
# Usage: scripts/probe-dac.sh > dac-report.txt   (share the file in a hardware report)
set -uo pipefail

section() { printf '\n## %s\n\n' "$1"; }

section "System"
echo "Kernel: $(uname -r)"
head -n1 /proc/asound/version
aplay --version 2>/dev/null | head -n1
command -v pw-cli >/dev/null && pw-cli --version 2>/dev/null | tail -n1

section "ALSA playback devices (aplay -l)"
aplay -l 2>&1 | grep -v Subdevice

found=0
for card in /proc/asound/card[0-9]*; do
    [[ -e "$card/stream0" ]] || continue   # USB audio devices only
    found=1
    index=${card##*card}
    usbid=$(cat "$card/usbid" 2>/dev/null)

    section "Card $index: $(cat "$card/id") ($usbid)"
    echo '```'
    lsusb -v -d "$usbid" 2>/dev/null \
        | grep -E 'bcdUSB|bcdDevice|iManufacturer|iProduct' \
        | sed 's/^ *//'
    echo
    cat "$card/stream0"
    echo
    echo "# Hardware mixer (amixer -c $index)"
    amixer -c "$index" 2>&1
    echo
    echo "# PCM status (closed = not held by any process)"
    for status in "$card"/pcm*p/sub0/status; do
        printf '%s: %s\n' "${status#/proc/asound/}" "$(head -n1 "$status")"
    done
    echo '```'
done

[[ $found -eq 1 ]] || echo "No USB audio device found. Is the DAC connected and powered?"

if command -v wpctl >/dev/null; then
    section "PipeWire sinks (wpctl status)"
    echo '```'
    wpctl status 2>/dev/null | sed -n '/Sinks:/,/Sources:/p'
    echo '```'
fi

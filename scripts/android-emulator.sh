#!/usr/bin/env bash
# Starts the Mirror Android emulator without letting it eat the machine.
#
# What makes the stock launch expensive, and what this does about it:
#   - software GL (swiftshader) renders every frame on the CPU  -> `-gpu host` uses the real GPU
#   - the AVD asks for 4 cores and the guest spins a lot at idle  -> hard CPU cap via a systemd scope
#   - audio/camera/metrics threads nobody uses                    -> disabled
#   - full cold boots                                            -> snapshot load (state is not saved back)
#
# Usage: scripts/android-emulator.sh [start|stop|status]   (env: AVD, CORES, MEMORY_MB, GPU, CPU_CAP)
set -euo pipefail

AVD="${AVD:-MirrorApi36}"
CORES="${CORES:-2}"            # guest vCPUs; 2 is plenty for a photo app
MEMORY_MB="${MEMORY_MB:-3072}"
GPU="${GPU:-host}"             # host | angle_indirect | swiftshader_indirect (CPU, last resort)
CPU_CAP="${CPU_CAP:-250%}"     # whole-emulator ceiling: 100% = one core
SDK="${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}"
export ANDROID_AVD_HOME="${ANDROID_AVD_HOME:-$HOME/.config/.android/avd}"
UNIT="mirror-emulator"
ADB="$SDK/platform-tools/adb"

running() { systemctl --user is-active --quiet "$UNIT.scope" 2>/dev/null || pgrep -f "qemu.*-avd $AVD" >/dev/null; }

case "${1:-start}" in
  status)
    if running; then
      echo "running"
      ps -o pid,pcpu,rss,nlwp,args -C qemu-system-x86_64 2>/dev/null | grep -- "-avd $AVD" | cut -c1-120 || true
      "$ADB" devices | tail -n +2
    else
      echo "stopped"
    fi
    ;;
  stop)
    "$ADB" emu kill >/dev/null 2>&1 || true
    # Wait for a clean exit, so an immediate `start` never sees the dying process.
    for _ in $(seq 1 30); do pgrep -f "qemu.*-avd $AVD" >/dev/null || break; sleep 1; done
    if pgrep -f "qemu.*-avd $AVD" >/dev/null; then
      systemctl --user kill --signal=KILL "$UNIT.scope" >/dev/null 2>&1 || pkill -9 -f "qemu.*-avd $AVD" || true
      sleep 1
    fi
    systemctl --user stop "$UNIT.scope" >/dev/null 2>&1 || true
    echo "stopped"
    ;;
  start)
    if running; then echo "already running"; exit 0; fi
    # `systemd-run --scope` puts the emulator in a cgroup: a hard CPU cap, low weight, and
    # a nice level so it yields to whatever you are actually working in.
    systemd-run --user --scope --quiet --unit="$UNIT" \
      -p CPUQuota="$CPU_CAP" -p CPUWeight=40 -p MemoryHigh="$((MEMORY_MB + 1536))M" \
      nice -n 10 \
      "$SDK/emulator/emulator" -avd "$AVD" \
        -gpu "$GPU" -cores "$CORES" -memory "$MEMORY_MB" \
        -no-audio -no-boot-anim -no-metrics -camera-back none \
        -netfast -no-snapshot-save \
        >"${TMPDIR:-/tmp}/mirror-emulator.log" 2>&1 &
    echo "starting $AVD (gpu=$GPU cores=$CORES mem=${MEMORY_MB}M cap=$CPU_CAP)"
    "$ADB" wait-for-device
    until [ "$("$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do sleep 1; done
    echo "booted"
    ;;
  *) echo "usage: $0 [start|stop|status]" >&2; exit 2 ;;
esac

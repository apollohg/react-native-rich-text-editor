#!/usr/bin/env bash
set -euo pipefail

mkdir -p test-results/ios

xcodebuild -version
runtimes="$(xcrun simctl list runtimes available)"
device_types="$(xcrun simctl list devicetypes)"
devices="$(xcrun simctl list devices available)"

printf '%s\n' "$runtimes"
printf '%s\n' "$device_types"

simulator_id="$(
  printf '%s\n' "$devices" \
    | sed -nE 's/^[[:space:]]+iPhone[^()]* \(([0-9A-F-]+)\) \((Booted|Shutdown|Creating)\)[[:space:]]*$/\1/p' \
    | head -n 1
)"

if [ -z "$simulator_id" ]; then
  device_type_identifier=""
  for preferred_name in \
    "iPhone 16" \
    "iPhone 16 Pro" \
    "iPhone 16 Pro Max" \
    "iPhone 16 Plus" \
    "iPhone 16e" \
    "iPhone 15"
  do
    device_type_identifier="$(
      printf '%s\n' "$device_types" \
        | sed -nE "s/^${preferred_name} \\((com\\.apple\\.CoreSimulator\\.SimDeviceType\\.[^)]+)\\)$/\\1/p" \
        | head -n 1
    )"
    if [ -n "$device_type_identifier" ]; then
      break
    fi
  done

  if [ -z "$device_type_identifier" ]; then
    device_type_identifier="$(
      printf '%s\n' "$device_types" \
        | sed -nE 's/^iPhone[^()]* \((com\.apple\.CoreSimulator\.SimDeviceType\.[^)]+)\)$/\1/p' \
        | head -n 1
    )"
  fi

  if [ -z "$device_type_identifier" ]; then
    echo "No iPhone simulator device type was found on this runner." >&2
    exit 1
  fi

  simulator_id="$(
    xcrun simctl create "NativeEditor CI iPhone" "$device_type_identifier"
  )"
fi

xcrun simctl boot "$simulator_id" || true
xcrun simctl bootstatus "$simulator_id" -b

echo "Using simulator destination id=$simulator_id"
echo "destination=id=$simulator_id" >> "$GITHUB_OUTPUT"

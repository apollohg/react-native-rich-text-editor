#!/usr/bin/env bash
set -euo pipefail

if [[ ! -e /dev/kvm ]]; then
  echo 'Android emulator requires /dev/kvm; enable KVM on the runner host.' >&2
  exit 1
fi

if [[ ! -r /dev/kvm || ! -w /dev/kvm ]]; then
  echo "Android emulator requires read/write access to /dev/kvm for $(id -un); grant the runner user access and restart the runner service." >&2
  exit 1
fi

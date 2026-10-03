#!/usr/bin/env bash

set -euo pipefail

test_class=""
configuration=""
forwarded=()
while (($# > 0)); do
  case "$1" in
    --class|--configuration)
      if (($# < 2)); then
        echo "$1 requires a value" >&2
        exit 2
      fi
      if [[ "$1" == "--class" ]]; then test_class="$2"; else configuration="$2"; fi
      shift 2
      ;;
    --) shift; forwarded+=("$@"); break ;;
    *) forwarded+=("$1"); shift ;;
  esac
done
if [[ -n "$configuration" && "$configuration" != "Debug" && "$configuration" != "Release" ]]; then
  echo "--configuration must be Debug or Release" >&2
  exit 2
fi
if [[ "$test_class" == "TablePerformanceTests" ]]; then
  export NATIVE_EDITOR_IOS_TEST_SCHEME="${NATIVE_EDITOR_IOS_TEST_SCHEME:-NativeEditorPreparedProsePerformance}"
fi
if [[ -n "$test_class" ]]; then forwarded+=("-only-testing:NativeEditorTests/$test_class"); fi
if [[ -n "$configuration" ]]; then forwarded+=("-configuration" "$configuration"); fi

local_env_file="ios-tests/.device-test.env"
if [[ -f "$local_env_file" ]]; then
  # shellcheck disable=SC1090
  source "$local_env_file"
fi

: "${IOS_DEVICE_ID:?Set IOS_DEVICE_ID or create ios-tests/.device-test.env}"
: "${IOS_DEVELOPMENT_TEAM:?Set IOS_DEVELOPMENT_TEAM or create ios-tests/.device-test.env}"

bash ./scripts/run-ios-tests.sh \
  --device-id "$IOS_DEVICE_ID" \
  --team-id "$IOS_DEVELOPMENT_TEAM" \
  "${forwarded[@]}"

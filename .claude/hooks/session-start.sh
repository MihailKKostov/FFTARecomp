#!/bin/bash
# Cloud sessions only: install what AndroidPort/scripts/build_android.py needs.
set -euo pipefail
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

SDK=/opt/android-sdk
NDK_VERSION=28.2.13676358
TOOLS_ZIP=commandlinetools-linux-13114758_latest.zip

if [ ! -x "$SDK/cmdline-tools/latest/bin/sdkmanager" ]; then
  mkdir -p "$SDK/cmdline-tools"
  tmp=$(mktemp -d)
  curl -sSfL -o "$tmp/tools.zip" "https://dl.google.com/android/repository/$TOOLS_ZIP"
  unzip -q "$tmp/tools.zip" -d "$tmp"
  rm -rf "$SDK/cmdline-tools/latest"
  mv "$tmp/cmdline-tools" "$SDK/cmdline-tools/latest"
  rm -rf "$tmp"
fi

if [ ! -d "$SDK/ndk/$NDK_VERSION" ] || [ ! -d "$SDK/platforms/android-35" ] || [ ! -d "$SDK/build-tools/35.0.0" ]; then
  yes | "$SDK/cmdline-tools/latest/bin/sdkmanager" --licenses >/dev/null 2>&1 || true
  "$SDK/cmdline-tools/latest/bin/sdkmanager" "platform-tools" "platforms;android-35" \
    "build-tools;35.0.0" "ndk;$NDK_VERSION" >/dev/null
fi

rustup target add aarch64-linux-android >/dev/null 2>&1

if [ -n "${CLAUDE_ENV_FILE:-}" ]; then
  echo "export ANDROID_HOME=$SDK" >> "$CLAUDE_ENV_FILE"
  echo "export ANDROID_SDK_ROOT=$SDK" >> "$CLAUDE_ENV_FILE"
fi

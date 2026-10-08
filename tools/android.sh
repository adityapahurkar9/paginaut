#!/usr/bin/env bash
# Builds the Android viewer and installs it on the running emulator or connected phone.
#
# Tauri puts the Rust library into the Gradle project with a symlink, which Windows only allows in
# Developer Mode; this copies it instead, then runs Gradle without Tauri's own Rust step.
#
# Usage: bash tools/android.sh [x86_64|arm64|arm|x86] [--release] [--no-install]
#        bash tools/android.sh all --release      the Play Store bundle (.aab, all four ABIs)
# Release builds are signed with the upload key %USERPROFILE%\.paginaut\android-signing.properties names (or the
# file PAGINAUT_ANDROID_SIGNING names; see src-tauri/gen/android/app/build.gradle.kts), and stop without it.
set -euo pipefail
cd "$(dirname "$0")/.."

arch=x86_64
profile=debug
install=1
for a in "$@"; do
  case "$a" in
    --release) profile=release ;;
    --no-install) install=0 ;;
    x86_64|arm64|arm|x86|all) arch=$a ;;
    *) echo "unknown argument: $a" >&2; exit 1 ;;
  esac
done

# Windows paths: Tauri's NDK check fails on some targets with /c/... ones.
export JAVA_HOME="${JAVA_HOME:-E:\Android Studio\jbr}"
export ANDROID_HOME="${ANDROID_HOME:-$LOCALAPPDATA\Android\Sdk}"
export NDK_HOME="${NDK_HOME:-$ANDROID_HOME\ndk\29.0.14206865}"
sdk=$(cygpath -u "$ANDROID_HOME")
ndk=$(cygpath -u "$NDK_HOME")
adb="$sdk/platform-tools/adb"
flag=$([ "$profile" = debug ] && echo --debug || echo "")

# Gradle would quietly make an unsigned release, which only shows when Play turns it down.
if [ "$profile" = release ]; then
  signing="${PAGINAUT_ANDROID_SIGNING:-$USERPROFILE/.paginaut/android-signing.properties}"
  if ! grep -qs "^storeFile=" "$(cygpath -u "$signing")"; then
    echo "Release builds need the upload key: $signing (storeFile, storePassword, keyAlias, keyPassword) wasn't found." >&2
    exit 1
  fi
fi
# The page is the same for every ABI: built with the first, then left as it is.
page=""

# One ABI's Rust library (and the frontend), copied where Gradle packs it.
build_lib() {
  local target triple abi
  case "$1" in
    x86_64) target=x86_64; triple=x86_64-linux-android; abi=x86_64 ;;
    arm64) target=aarch64; triple=aarch64-linux-android; abi=arm64-v8a ;;
    arm) target=armv7; triple=armv7-linux-androideabi; abi=armeabi-v7a ;;
    x86) target=i686; triple=i686-linux-android; abi=x86 ;;
  esac
  # Tauri stops at the symlink without Developer Mode.
  npx tauri android build $flag --apk --target "$target" $page > "${TEMP:-/tmp}/paginaut-android.log" 2>&1 || true
  if ! grep -q "symlinking lib\|BUILD SUCCESSFUL" "${TEMP:-/tmp}/paginaut-android.log"; then
    tail -40 "${TEMP:-/tmp}/paginaut-android.log"
    exit 1
  fi
  local jni="src-tauri/gen/android/app/src/main/jniLibs/$abi"
  rm -rf "$jni"
  mkdir -p "$jni"
  # Debug info makes the library ~200 MB (slow to install, very slow on Android 7's adb); stack
  # traces still name functions without it.
  "$ndk/toolchains/llvm/prebuilt/windows-x86_64/bin/llvm-strip.exe" --strip-debug -o "$jni/libpaginaut_lib.so" "src-tauri/target/$triple/$profile/libpaginaut_lib.so"
  page='--config {"build":{"beforeBuildCommand":""}}'
}

Profile="$(tr '[:lower:]' '[:upper:]' <<< "${profile:0:1}")${profile:1}"

if [ "$arch" = all ]; then
  for a in arm64 arm x86 x86_64; do build_lib "$a"; done
  rm -rf "src-tauri/gen/android/app/build/outputs/bundle/universal$Profile"
  (cd src-tauri/gen/android && ./gradlew "bundleUniversal$Profile" -x "rustBuildUniversal$Profile" --console=plain -q)
  aab=$(ls src-tauri/gen/android/app/build/outputs/bundle/universal"$Profile"/*.aab | head -1)
  echo "AAB: $aab ($(du -h "$aab" | cut -f1))"
  exit 0
fi

build_lib "$arch"
Arch="$(tr '[:lower:]' '[:upper:]' <<< "${arch:0:1}")${arch:1}"
# Debug builds rewrite the APK in place, keeping old bytes: start from none.
rm -rf "src-tauri/gen/android/app/build/outputs/apk/$arch/$profile"
(cd src-tauri/gen/android && ./gradlew "assemble$Arch$Profile" -x "rustBuild$Arch$Profile" --console=plain -q)
apk=$(ls src-tauri/gen/android/app/build/outputs/apk/"$arch"/"$profile"/*.apk | head -1)
echo "APK: $apk ($(du -h "$apk" | cut -f1))"
[ "$install" = 1 ] && "$adb" install -r "$apk"

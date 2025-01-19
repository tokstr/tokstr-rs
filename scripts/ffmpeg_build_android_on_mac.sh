#!/usr/bin/env bash
set -euo pipefail

################################################################################
# Configuration
################################################################################

# Default API level if not defined externally
: "${API:=21}"

# Default NDK path if not defined externally (Adjust NDK version/path as needed)
NDK_PATH="${NDK:-"$HOME/Library/Android/sdk/ndk/26.1.10909125"}"
HOST_TAG="darwin-x86_64"
ARCH="aarch64"

# The first argument to this script is the output directory (relative or absolute)
OUTPUT_DIR="${1:-"$(pwd)/ffmpeg-libs"}"

################################################################################
# Preliminary checks
################################################################################

# Make sure the NDK path exists
if [[ ! -d "$NDK_PATH" ]]; then
  echo "ERROR: Android NDK not found at: $NDK_PATH"
  exit 1
fi

# Required commands
for cmd in git sysctl; do
  if ! command -v "$cmd" &>/dev/null; then
    echo "ERROR: '$cmd' is required but not found in PATH."
    exit 1
  fi
done

################################################################################
# Create a temporary workspace
################################################################################

BUILD_DIR="$(mktemp -d -t ffmpeg-build-XXXX)"
FFMPEG_SRC="$BUILD_DIR/ffmpeg"
INSTALL_PREFIX="$BUILD_DIR/arm64-v8a"

echo "Created temporary build directory: $BUILD_DIR"
mkdir -p "$INSTALL_PREFIX"

################################################################################
# Toolchain configuration
################################################################################

TOOLCHAIN="$NDK_PATH/toolchains/llvm/prebuilt/$HOST_TAG"
SYSROOT="$TOOLCHAIN/sysroot"

CC="$TOOLCHAIN/bin/${ARCH}-linux-android${API}-clang"
CXX="$TOOLCHAIN/bin/${ARCH}-linux-android${API}-clang++"
AR="$TOOLCHAIN/bin/llvm-ar"
LD="$TOOLCHAIN/bin/ld.lld"
STRIP="$TOOLCHAIN/bin/llvm-strip"
NM="$TOOLCHAIN/bin/llvm-nm"
RANLIB="$TOOLCHAIN/bin/llvm-ranlib"

export PATH="$TOOLCHAIN/bin:$PATH"

if ! command -v "$CC" &>/dev/null; then
  echo "ERROR: Compiler not found at $CC"
  exit 1
fi

echo "Using compiler: $(which "$CC")"
"$CC" --version

################################################################################
# Fetch FFmpeg source
################################################################################

echo "Cloning FFmpeg into $FFMPEG_SRC"
git clone --depth=1 https://github.com/FFmpeg/FFmpeg.git "$FFMPEG_SRC"

################################################################################
# Configure FFmpeg
################################################################################

echo "Configuring FFmpeg for Android arm64..."
cd "$FFMPEG_SRC"

./configure \
  --prefix="$INSTALL_PREFIX" \
  --enable-cross-compile \
  --target-os=android \
  --arch=arm64 \
  --cpu=armv8-a \
  --cc="$CC" \
  --cxx="$CXX" \
  --nm="$NM" \
  --ranlib="$RANLIB" \
  --ar="$AR" \
  --strip="$STRIP" \
  --sysroot="$SYSROOT" \
  --cross-prefix="$TOOLCHAIN/bin/${ARCH}-linux-android-" \
  --enable-pic \
  --enable-static \
  --disable-shared \
  --disable-programs \
  --disable-doc \
  --disable-symver \
  --enable-gpl \
  --enable-nonfree \
  --enable-pthreads

################################################################################
# Build & Install
################################################################################

echo "Building FFmpeg..."
make -j"$(sysctl -n hw.logicalcpu)"

echo "Installing FFmpeg to $INSTALL_PREFIX..."
make install

################################################################################
# Copy artifacts to the output directory
################################################################################

# IMPORTANT: $OUTPUT_DIR could be relative. We can ensure it is an absolute
# path if you prefer, or just create it as-is.
echo "Copying artifacts to $OUTPUT_DIR..."
mkdir -p "$OUTPUT_DIR"
cp -R "$INSTALL_PREFIX"/* "$OUTPUT_DIR"

echo "FFmpeg build artifacts are now located in: $OUTPUT_DIR"
echo "Cleaning up temporary directory $BUILD_DIR..."
rm -rf "$BUILD_DIR"

echo "Done."
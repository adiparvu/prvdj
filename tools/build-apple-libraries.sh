#!/usr/bin/env bash
#
# Builds the core as something an Apple application can actually ship.
#
# # Why this exists
#
# `Package.swift` links `-L core/target/debug`, which is exactly right for a
# developer on one machine and useless for a release. That directory holds one
# architecture, built for debugging, for whatever host ran `cargo`. A shipped
# macOS application has to run on Apple silicon and Intel; an iOS application
# needs one slice for devices and another for the simulator, and the simulator
# slice is a different target triple rather than a different build of the same
# one.
#
# An `.xcframework` is the container that holds those slices and lets the linker
# pick. Building one is four cargo invocations, two `lipo` merges and an
# `xcodebuild` call — mechanical, easy to get subtly wrong by hand, and
# therefore written down once here.
#
# # Why the header travels with it
#
# `bridgegen` produces `PRVBridge.h` and a module map, and architecture rule 9
# asserts the committed copies are current. The framework carries them so that a
# consumer cannot end up with a library from one commit and a header from
# another — the drift that rule 9 exists to prevent, reappearing at a different
# layer.
#
# # Requires
#
# A Mac with Xcode. Nothing here runs on Linux, and it says so rather than
# producing something that looks like a framework and is not.

set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
core="$root/core"
staging="$core/target/apple"
out="$root/apple/PRVKit/Bridge/Generated/PRVBridge.xcframework"

# Release, always. A debug build of the core is between five and twenty times
# slower depending on the routine, and the audio thread has a deadline that does
# not care why it was missed.
profile="release"

if [ "$(uname -s)" != "Darwin" ]; then
    echo "This needs a Mac: the simulator and device slices are built with Apple's" >&2
    echo "toolchain, and an .xcframework is assembled by xcodebuild." >&2
    exit 1
fi

command -v cargo >/dev/null || { echo "cargo is not on the path" >&2; exit 1; }
command -v xcodebuild >/dev/null || { echo "xcodebuild is not on the path" >&2; exit 1; }

targets=(
    aarch64-apple-darwin
    x86_64-apple-darwin
    aarch64-apple-ios
    aarch64-apple-ios-sim
    x86_64-apple-ios
)

echo "==> Installing target toolchains"
for target in "${targets[@]}"; do
    rustup target add "$target" >/dev/null
done

echo "==> Building the boundary for ${#targets[@]} targets"
for target in "${targets[@]}"; do
    cargo build --manifest-path "$core/Cargo.toml" -p prv-ffi --profile "$profile" --target "$target"
done

echo "==> Regenerating the header, so the framework cannot carry a stale one"
cargo run --manifest-path "$core/Cargo.toml" -q -p prv-ffi --bin bridgegen -- \
    "$root/apple/PRVKit/Bridge/Generated/PRVBridge.h"

headers="$staging/Headers"
rm -rf "$staging"
mkdir -p "$staging" "$headers"
cp "$root/apple/PRVKit/Bridge/Generated/PRVBridge.h" "$headers/"
cp "$root/apple/PRVKit/Bridge/Generated/module.modulemap" "$headers/"

lib="libprv_ffi.a"
slice() { echo "$core/target/$1/$profile/$lib"; }

# macOS is one universal slice; the App Store takes a single binary that runs on
# both architectures rather than two submissions.
echo "==> Merging the macOS architectures"
mkdir -p "$staging/macos"
lipo -create "$(slice aarch64-apple-darwin)" "$(slice x86_64-apple-darwin)" \
    -output "$staging/macos/$lib"

# The device slice stands alone: there is only one iOS device architecture.
mkdir -p "$staging/ios"
cp "$(slice aarch64-apple-ios)" "$staging/ios/$lib"

# The simulator slices merge, because a developer may be on either kind of Mac.
echo "==> Merging the simulator architectures"
mkdir -p "$staging/ios-simulator"
lipo -create "$(slice aarch64-apple-ios-sim)" "$(slice x86_64-apple-ios)" \
    -output "$staging/ios-simulator/$lib"

echo "==> Assembling the framework"
rm -rf "$out"
xcodebuild -create-xcframework \
    -library "$staging/macos/$lib" -headers "$headers" \
    -library "$staging/ios/$lib" -headers "$headers" \
    -library "$staging/ios-simulator/$lib" -headers "$headers" \
    -output "$out"

echo
echo "Wrote $out"
lipo -info "$staging/macos/$lib" "$staging/ios-simulator/$lib" 2>/dev/null || true

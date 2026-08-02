// swift-tools-version: 6.0
//
// The Apple side of PRV AI DJ Studio.
//
// # Every target builds on Linux, and that is the point
//
// An earlier version of this manifest declared `PRVKit` and `PRVUI` only on
// macOS, because they are where CoreAudio and SwiftUI arrive. The effect was
// that everything in them — including the logic that has nothing to do with
// either — went unbuilt and untested on every commit.
//
// So the split is no longer by target. It is by `#if canImport`, *inside* the
// targets, around the smallest possible amount of code:
//
//   - The ports, the session that wires the core together, the decoders that
//     work on any platform, and every view model: plain Swift, built and tested
//     on Linux on every commit.
//   - The CoreAudio render host, the AVFoundation decoder, the keychain store
//     and the SwiftUI views: behind `#if canImport`, absent on Linux, and
//     deliberately thin — each is an adapter over a protocol that is itself
//     tested.
//
// The rule that keeps this honest: **no decision lives inside a `#if`.** If a
// conditional block contains anything worth testing, it is in the wrong place.

import PackageDescription

let package = Package(
    name: "PRV",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "PRVCore", targets: ["PRVCore"]),
        .library(name: "PRVKit", targets: ["PRVKit"]),
        .library(name: "PRVUI", targets: ["PRVUI"]),
    ],
    targets: [
        // The generated C boundary. `path` points at what `bridgegen` produces,
        // so there is no copy of the header to fall behind.
        .systemLibrary(
            name: "CPRVBridge",
            path: "PRVKit/Bridge/Generated"
        ),

        .target(
            name: "PRVCore",
            dependencies: ["CPRVBridge"],
            path: "Sources/PRVCore",
            linkerSettings: [
                // Where cargo leaves the static library. Both profiles are
                // offered because a developer builds debug and continuous
                // integration builds release, and neither should have to edit
                // this file to run the tests.
                .unsafeFlags([
                    "-L../core/target/debug",
                    "-L../core/target/release",
                ])
            ]
        ),

        .target(name: "PRVKit", dependencies: ["PRVCore"], path: "Sources/PRVKit"),
        .target(name: "PRVUI", dependencies: ["PRVCore", "PRVKit"], path: "Sources/PRVUI"),

        .testTarget(
            name: "PRVCoreTests",
            dependencies: ["PRVCore"],
            path: "Tests/PRVCoreTests"
        ),
        .testTarget(
            name: "PRVKitTests",
            dependencies: ["PRVKit"],
            path: "Tests/PRVKitTests"
        ),
        .testTarget(
            name: "PRVUITests",
            dependencies: ["PRVUI"],
            path: "Tests/PRVUITests"
        ),
    ]
)

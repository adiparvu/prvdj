// swift-tools-version: 6.0
//
// The Apple side of PRV AI DJ Studio.
//
// # Why the packages split where they do
//
// `PRVCore` wraps the C boundary and depends on nothing but Foundation. That is
// deliberate and it is the most useful property in this file: it means the layer
// where every mistake is unrecoverable — pointer lifetimes, error codes, the
// audio callback — compiles and runs on Linux, in continuous integration, on
// every commit.
//
// `PRVKit` is where the Apple frameworks arrive: CoreAudio, AVFoundation, the
// keychain, security-scoped bookmarks. Those cannot be built without the Apple
// SDKs, so the target is declared for platforms that have them and is absent
// elsewhere. Keeping it separate from `PRVCore` is what stops the untestable
// half from swallowing the testable half.
//
// `PRVUI` is SwiftUI and holds no business rules, as the architecture overview
// requires of the presentation layer.

import PackageDescription

#if os(macOS)
let applePlatformTargets: [Target] = [
    .target(
        name: "PRVKit",
        dependencies: ["PRVCore"],
        path: "Sources/PRVKit"
    ),
    .target(
        name: "PRVUI",
        dependencies: ["PRVCore", "PRVKit"],
        path: "Sources/PRVUI"
    ),
]
let applePlatformProducts: [Product] = [
    .library(name: "PRVKit", targets: ["PRVKit"]),
    .library(name: "PRVUI", targets: ["PRVUI"]),
]
#else
let applePlatformTargets: [Target] = []
let applePlatformProducts: [Product] = []
#endif

let package = Package(
    name: "PRV",
    platforms: [.macOS(.v14), .iOS(.v17)],
    products: [
        .library(name: "PRVCore", targets: ["PRVCore"])
    ] + applePlatformProducts,
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

        .testTarget(
            name: "PRVCoreTests",
            dependencies: ["PRVCore"],
            path: "Tests/PRVCoreTests"
        ),
    ] + applePlatformTargets
)

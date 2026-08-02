import Foundation
import PRVCore
import PRVKit
import PRVUI

// The application.
//
// # Why this file has two halves
//
// On Apple platforms it is a SwiftUI app. Everywhere else it is a command-line
// programme that starts the same session, imports nothing, and reports what it
// found — which sounds pointless and is the most useful test in the repository:
// it is the only thing that proves the whole stack *links into an executable*.
//
// A library that compiles and a binary that starts are different claims. Every
// other test here exercises a module; this one exercises the product.

#if canImport(SwiftUI)
    import SwiftUI

    @main
    struct PRVStudioApp: App {
        @State private var studio = Studio()

        var body: some Scene {
            WindowGroup {
                StudioWindow(
                    library: studio.library,
                    planning: studio.planning,
                    transport: studio.transport,
                    onPlay: { studio.play() },
                    onPause: { studio.pause() },
                    onSelect: { studio.select($0) },
                    onAdopt: { studio.adopt($0) }
                )
                .task { studio.start() }
            }
            .commands {
                CommandGroup(replacing: .newItem) {}
            }
        }
    }
#else
    // A headless start. Exits non-zero if the stack cannot be brought up, which
    // is what makes this worth running in continuous integration.
    let studio = Studio()
    studio.start()

    guard studio.isReady else {
        FileHandle.standardError.write(Data("the studio would not start\n".utf8))
        exit(1)
    }
    print("PRV AI DJ Studio")
    print("  boundary version \(Engine.abiVersion >> 16).\((Engine.abiVersion >> 8) & 0xFF)")
    print("  library: \(studio.library.rows.count) tracks")
    print("  transport: \(studio.transport.stateKey)")
    print("  anything leaves the device: \(studio.sendsAnything)")
#endif

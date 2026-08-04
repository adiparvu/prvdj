import Foundation
import PRVCore

// MARK: - Decoding

/// Decodes with AVFoundation on Apple platforms.
///
/// # Authored, not verified
///
/// This is one of four types in the package that import an Apple framework, and
/// none of them can be compiled by the continuous integration available to this
/// project (see `docs/mts/17-known-limitations.md`, R-01). It is written to be
/// as small as a working adapter can be, precisely because it is the code that
/// nothing checks: every decision it might have made lives in ``Session``, which
/// is tested.
#if canImport(AVFoundation)
    import AVFoundation

    public struct AVFoundationDecoder: MediaDecoder {
        public init() {}

        public func decodeMono(_ item: MediaItem, sampleRate: UInt32) throws -> [Float] {
            let file: AVAudioFile
            do {
                file = try AVAudioFile(forReading: item.location)
            } catch {
                throw PlatformError.cannotRead(item.location.lastPathComponent)
            }

            guard
                let format = AVAudioFormat(
                    commonFormat: .pcmFormatFloat32,
                    sampleRate: Double(sampleRate),
                    channels: 1,
                    interleaved: false
                )
            else {
                throw PlatformError.unsupportedFormat("mono float at \(sampleRate) Hz")
            }

            guard let converter = AVAudioConverter(from: file.processingFormat, to: format) else {
                throw PlatformError.unsupportedFormat(file.processingFormat.description)
            }

            let capacity = AVAudioFrameCount(
                Double(file.length) * Double(sampleRate) / file.processingFormat.sampleRate + 1
            )
            guard
                let output = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: max(capacity, 1))
            else {
                throw PlatformError.cannotRead(item.location.lastPathComponent)
            }

            var done = false
            var conversionError: NSError?
            converter.convert(to: output, error: &conversionError) { _, status in
                if done {
                    status.pointee = .endOfStream
                    return nil
                }
                guard
                    let input = AVAudioPCMBuffer(
                        pcmFormat: file.processingFormat,
                        frameCapacity: 4_096
                    )
                else {
                    status.pointee = .endOfStream
                    return nil
                }
                do {
                    try file.read(into: input)
                } catch {
                    status.pointee = .endOfStream
                    return nil
                }
                if input.frameLength == 0 {
                    done = true
                    status.pointee = .endOfStream
                    return nil
                }
                status.pointee = .haveData
                return input
            }

            if conversionError != nil {
                throw PlatformError.unsupportedFormat(item.location.lastPathComponent)
            }
            guard let channel = output.floatChannelData?[0] else { return [] }
            return Array(UnsafeBufferPointer(start: channel, count: Int(output.frameLength)))
        }
    }
#endif

// MARK: - Output

/// Sends blocks to the default output device with AVAudioEngine.
///
/// Authored, not verified — see ``AVFoundationDecoder``.
#if canImport(AVFoundation)
    import AVFoundation

    public final class CoreAudioOutput: AudioOutput {
        private let engine = AVAudioEngine()
        private var sourceNode: AVAudioSourceNode?
        /// The render scratch, held so that `stop` can return it.
        ///
        /// It used to be a local, freed only on the error path — so every
        /// successful start-and-stop leaked one block, and a set that switched
        /// devices a few times leaked a few. Holding it is what makes the
        /// lifetime match the node's.
        private var scratch: UnsafeMutableBufferPointer<Float>?
        public private(set) var isRunning = false

        public init() {}

        public func start(
            channels: Int,
            sampleRate: UInt32,
            render: @escaping @Sendable (UnsafeMutableBufferPointer<Float>, Int) -> Void
        ) throws {
            guard !isRunning else { return }
            try Self.prepareSession()
            guard
                let format = AVAudioFormat(
                    standardFormatWithSampleRate: Double(sampleRate),
                    channels: AVAudioChannelCount(channels)
                )
            else {
                throw PlatformError.deviceUnavailable
            }

            // A scratch block, allocated here and reused by every callback. The
            // render closure needs one contiguous channel-major buffer and
            // CoreAudio hands over a list of per-channel pointers, so the copy
            // below is the price of the boundary's shape. It is a memcpy of one
            // block and allocates nothing.
            let maxFrames = 4_096
            let scratch = UnsafeMutableBufferPointer<Float>.allocate(
                capacity: channels * maxFrames
            )
            scratch.initialize(repeating: 0)
            self.scratch = scratch

            let node = AVAudioSourceNode(format: format) { _, _, frameCount, audioBufferList in
                let frames = min(Int(frameCount), maxFrames)
                render(scratch, frames)

                let buffers = UnsafeMutableAudioBufferListPointer(audioBufferList)
                for (channel, buffer) in buffers.enumerated() where channel < channels {
                    guard let destination = buffer.mData?.assumingMemoryBound(to: Float.self)
                    else { continue }
                    let source = scratch.baseAddress?.advanced(by: channel * maxFrames)
                    if let source {
                        destination.update(from: source, count: frames)
                    }
                }
                return noErr
            }

            engine.attach(node)
            engine.connect(node, to: engine.mainMixerNode, format: format)
            sourceNode = node

            do {
                try engine.start()
            } catch {
                engine.detach(node)
                sourceNode = nil
                scratch.deallocate()
                self.scratch = nil
                throw PlatformError.deviceUnavailable
            }
            isRunning = true
        }

        public func stop() {
            guard isRunning else { return }
            engine.stop()
            if let sourceNode {
                engine.detach(sourceNode)
            }
            sourceNode = nil
            // After the node is detached, so nothing can be rendering into it.
            scratch?.deallocate()
            scratch = nil
            isRunning = false
        }

        deinit {
            // A caller that drops the output without stopping it should not
            // leak. Detaching is the engine's business and it is going away
            // too; the buffer is ours.
            scratch?.deallocate()
        }

        /// Puts the audio session into a state that plays.
        ///
        /// # Why only iOS has this
        ///
        /// macOS has no `AVAudioSession`: an application asks for an output
        /// device and gets one. On iOS the session is what decides whether
        /// audio plays at all, whether it survives the screen locking, and what
        /// happens when a call arrives — and the default category is
        /// `.soloAmbient`, which is silenced by the ring switch and stops in the
        /// background.
        ///
        /// A DJ application silenced by a hardware switch, or stopped because
        /// somebody answered a message, is not usable. `.playback` is the
        /// category that says so, and it is the counterpart of the `audio`
        /// background mode declared in the iOS Info.plist — neither works
        /// without the other.
        private static func prepareSession() throws {
            #if os(iOS)
                do {
                    let session = AVAudioSession.sharedInstance()
                    try session.setCategory(.playback, mode: .default)
                    try session.setActive(true)
                } catch {
                    throw PlatformError.deviceUnavailable
                }
            #endif
        }
    }
#endif

// MARK: - Secrets

/// Stores secrets in the keychain.
///
/// Authored, not verified — see ``AVFoundationDecoder``.
#if canImport(Security)
    import Security

    public struct KeychainStore: SecretStore {
        private let service: String

        public init(service: String = "studio.prv.aidj") {
            self.service = service
        }

        public func store(_ secret: Data, forKey key: String) throws {
            // Delete first: `SecItemAdd` fails on a duplicate, and an update
            // path that only sometimes runs is a path that is only sometimes
            // tested.
            try? removeSecret(forKey: key)
            let query: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: service,
                kSecAttrAccount as String: key,
                kSecValueData as String: secret,
                // Never synchronised, never readable while locked. A device
                // authorisation token is not something to leave on a backup.
                kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            ]
            let status = SecItemAdd(query as CFDictionary, nil)
            guard status == errSecSuccess else {
                throw PlatformError.secureStoreFailed("add returned \(status)")
            }
        }

        public func secret(forKey key: String) throws -> Data? {
            let query: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: service,
                kSecAttrAccount as String: key,
                kSecReturnData as String: true,
                kSecMatchLimit as String: kSecMatchLimitOne,
            ]
            var item: CFTypeRef?
            let status = SecItemCopyMatching(query as CFDictionary, &item)
            if status == errSecItemNotFound { return nil }
            guard status == errSecSuccess, let data = item as? Data else {
                throw PlatformError.secureStoreFailed("copy returned \(status)")
            }
            return data
        }

        public func removeSecret(forKey key: String) throws {
            let query: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: service,
                kSecAttrAccount as String: key,
            ]
            let status = SecItemDelete(query as CFDictionary)
            guard status == errSecSuccess || status == errSecItemNotFound else {
                throw PlatformError.secureStoreFailed("delete returned \(status)")
            }
        }
    }
#endif

// MARK: - Everywhere

/// Stores a project's log as a file.
///
/// Plain Foundation, so it works on every platform this package builds on and is
/// covered by the tests rather than by hope.
public struct FileProjectStore: ProjectStore {
    private let directory: URL

    public init(directory: URL) {
        self.directory = directory
    }

    private func url(for name: String) -> URL {
        directory.appendingPathComponent("\(name).prvlog")
    }

    public func save(_ data: Data, named name: String) throws {
        do {
            try FileManager.default.createDirectory(
                at: directory,
                withIntermediateDirectories: true
            )
            try data.write(to: url(for: name), options: .atomic)
        } catch {
            throw PlatformError.cannotRead(name)
        }
    }

    public func load(named name: String) throws -> Data? {
        let location = url(for: name)
        guard FileManager.default.fileExists(atPath: location.path) else { return nil }
        do {
            return try Data(contentsOf: location)
        } catch {
            throw PlatformError.cannotRead(name)
        }
    }

    public func names() throws -> [String] {
        guard
            let entries = try? FileManager.default.contentsOfDirectory(
                at: directory,
                includingPropertiesForKeys: nil
            )
        else { return [] }
        return entries
            .filter { $0.pathExtension == "prvlog" }
            .map { $0.deletingPathExtension().lastPathComponent }
            .sorted()
    }
}

/// Decodes from samples handed over in advance.
///
/// Not a stub. It is what a preview uses, what every test uses, and what proves
/// the workflow in ``Session`` has no hidden dependency on AVFoundation.
public struct InMemoryDecoder: MediaDecoder {
    private let tracks: [UInt64: [Float]]

    public init(tracks: [UInt64: [Float]]) {
        self.tracks = tracks
    }

    public func decodeMono(_ item: MediaItem, sampleRate: UInt32) throws -> [Float] {
        guard let samples = tracks[item.id] else {
            throw PlatformError.cannotRead(item.title)
        }
        return samples
    }
}

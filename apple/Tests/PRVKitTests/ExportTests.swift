import Foundation
import Testing

@testable import PRVCore
@testable import PRVKit

@Suite("Getting a set out as a file")
struct ExportTests {

    /// Reads a WAVE file back the way a player would.
    private struct Wave {
        let channels: UInt16
        let sampleRate: UInt32
        let bitsPerSample: UInt16
        let formatTag: UInt16
        let audio: [UInt8]
        let declaredRiffSize: UInt32
        let declaredDataSize: UInt32

        init?(_ data: Data) {
            let bytes = [UInt8](data)
            func u16(_ at: Int) -> UInt16 {
                UInt16(bytes[at]) | (UInt16(bytes[at + 1]) << 8)
            }
            func u32(_ at: Int) -> UInt32 {
                UInt32(bytes[at]) | (UInt32(bytes[at + 1]) << 8)
                    | (UInt32(bytes[at + 2]) << 16) | (UInt32(bytes[at + 3]) << 24)
            }
            guard bytes.count >= 44,
                Array(bytes[0..<4]) == Array("RIFF".utf8),
                Array(bytes[8..<12]) == Array("WAVE".utf8),
                Array(bytes[12..<16]) == Array("fmt ".utf8),
                Array(bytes[36..<40]) == Array("data".utf8)
            else { return nil }

            declaredRiffSize = u32(4)
            formatTag = u16(20)
            channels = u16(22)
            sampleRate = u32(24)
            bitsPerSample = u16(34)
            declaredDataSize = u32(40)
            audio = Array(bytes[44...])
        }
    }

    /// A session holding one track of a constant tone, placed on the timeline.
    private func setWithAudio(frames: Int64 = 12_000, value: Float = 0.5) throws -> Session {
        let samples = [Float](repeating: value, count: Int(frames))
        let session = try Session(
            decoder: InMemoryDecoder(tracks: [1: samples]),
            output: nil
        )
        try session.import(
            MediaItem(
                id: 1,
                title: "Tone",
                artist: "",
                location: URL(fileURLWithPath: "/tone.wav")
            )
        )
        try session.engineForTesting.placeTrack(track: 1, position: 0, length: frames)
        return session
    }

    private func temporaryURL() -> URL {
        FileManager.default.temporaryDirectory
            .appendingPathComponent("\(UUID().uuidString).wav")
    }

    @Test("an exported set is a file a player can read")
    func writesAReadableFile() throws {
        let session = try setWithAudio()
        let url = temporaryURL()
        defer { try? FileManager.default.removeItem(at: url) }

        let outcome = try session.export(to: url, depth: .twentyFour, dither: false)

        let wave = try #require(Wave(Data(contentsOf: url)))
        #expect(wave.channels == 2)
        #expect(wave.sampleRate == 48_000)
        #expect(wave.bitsPerSample == 24)
        #expect(wave.formatTag == 1, "an integer file must not claim to be float")
        #expect(UInt64(wave.audio.count) == outcome.bytes)
    }

    @Test("the header's sizes match what was actually written")
    func sizesArePatched() throws {
        // A file left unfinished has a header claiming zero bytes of audio,
        // which every player honours: the samples are all there and none play.
        let session = try setWithAudio()
        let url = temporaryURL()
        defer { try? FileManager.default.removeItem(at: url) }

        try session.export(to: url, depth: .sixteen, dither: false)
        let wave = try #require(Wave(Data(contentsOf: url)))

        #expect(wave.declaredDataSize == UInt32(wave.audio.count))
        #expect(wave.declaredRiffSize == 36 + UInt32(wave.audio.count))
    }

    @Test("the file is as long as the set, to the sample")
    func lengthMatchesTheSet() throws {
        // The last block is short. Rendering a whole one and trimming would put
        // up to a block of silence on the end of every export.
        let frames: Int64 = 12_000
        let session = try setWithAudio(frames: frames)
        let url = temporaryURL()
        defer { try? FileManager.default.removeItem(at: url) }

        try session.export(to: url, depth: .sixteen, dither: false, blockFrames: 512)
        let wave = try #require(Wave(Data(contentsOf: url)))

        let expected = Int(frames) * 2 * 2  // frames * channels * bytes
        #expect(wave.audio.count == expected)
    }

    @Test("a float export says it is float, and is not dithered")
    func floatFiles() throws {
        let session = try setWithAudio()
        let url = temporaryURL()
        defer { try? FileManager.default.removeItem(at: url) }

        let outcome = try session.export(to: url, depth: .float32, dither: true)
        let wave = try #require(Wave(Data(contentsOf: url)))

        #expect(wave.formatTag == 3, "a float file must not claim to be integers")
        #expect(wave.bitsPerSample == 32)
        #expect(!outcome.dithered, "dither into floating point damages a lossless format")
        #expect(outcome.isFloat)
    }

    @Test("the same set exported twice is the same file, dither and all")
    func reproducible() throws {
        // ADR-0006 requires a render to be reproducible. Dither is noise, and
        // noise from anywhere but a seed would make every export a different
        // file.
        let first = temporaryURL()
        let second = temporaryURL()
        defer {
            try? FileManager.default.removeItem(at: first)
            try? FileManager.default.removeItem(at: second)
        }

        try setWithAudio().export(to: first, depth: .sixteen, dither: true, seed: 42)
        try setWithAudio().export(to: second, depth: .sixteen, dither: true, seed: 42)

        #expect(try Data(contentsOf: first) == Data(contentsOf: second))
    }

    @Test("the audio in the file is the audio of the set")
    func theSamplesAreRight() throws {
        // A file that is the right length and full of silence passes every
        // structural check and is useless.
        let session = try setWithAudio(frames: 4_800, value: 0.5)
        let url = temporaryURL()
        defer { try? FileManager.default.removeItem(at: url) }

        try session.export(to: url, depth: .sixteen, dither: false)
        let wave = try #require(Wave(Data(contentsOf: url)))

        let samples = stride(from: 0, to: wave.audio.count, by: 2).map { at in
            Int16(bitPattern: UInt16(wave.audio[at]) | (UInt16(wave.audio[at + 1]) << 8))
        }
        let peak = samples.map { abs(Int($0)) }.max() ?? 0
        #expect(peak > 1_000, "the file is silent")
        #expect(peak <= 32_767)
    }

    @Test("stopping an export leaves no file rather than half a set")
    func cancellation() throws {
        // A partial file that looks finished is worse than none: somebody plays
        // it, hears it stop, and blames the set.
        let session = try setWithAudio(frames: 48_000)
        let url = temporaryURL()
        defer { try? FileManager.default.removeItem(at: url) }

        #expect(throws: (any Error).self) {
            try session.export(to: url, blockFrames: 512) { fraction in
                fraction < 0.2
            }
        }
        #expect(!FileManager.default.fileExists(atPath: url.path))
    }

    @Test("progress runs from something to one")
    func progressReported() throws {
        let session = try setWithAudio(frames: 24_000)
        let url = temporaryURL()
        defer { try? FileManager.default.removeItem(at: url) }

        var seen: [Double] = []
        try session.export(to: url, blockFrames: 4_096) { fraction in
            seen.append(fraction)
            return true
        }

        #expect(!seen.isEmpty)
        #expect(seen.last == 1.0, "the export finished without saying so")
        #expect(seen == seen.sorted(), "progress went backwards")
    }

    @Test("exporting an empty project is refused rather than writing a header")
    func nothingToExport() throws {
        let session = try Session(decoder: InMemoryDecoder(tracks: [:]), output: nil)
        let url = temporaryURL()

        #expect(throws: (any Error).self) {
            try session.export(to: url)
        }
    }

    @Test("the header is right without touching a filesystem")
    func headerFormat() {
        let header = WaveWriter.header(
            channels: 2, sampleRate: 44_100, bitsPerSample: 16, isFloat: false
        )
        let bytes = [UInt8](header)

        #expect(bytes.count == 44)
        #expect(Array(bytes[0..<4]) == Array("RIFF".utf8))
        // Block align: 2 channels of 2 bytes.
        #expect(bytes[32] == 4)
        // Bytes per second: 44100 * 4 = 176400 = 0x0002B110.
        #expect(WaveWriter.littleEndian(UInt32(176_400)) == Array(bytes[28..<32]))
    }
}

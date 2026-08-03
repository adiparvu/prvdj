import Foundation
import Testing

@testable import PRVCore
@testable import PRVUI

@Suite("What the cloud looks like from the outside")
struct CloudModelTests {

    // MARK: - Consent

    @Test("a fresh installation says nothing leaves the device, and means it")
    func nothingAgreedToYet() throws {
        let policy = try Policy()
        let model = try ConsentModel(policy: policy)

        #expect(model.rows.count == Purpose.allCases.count, "a purpose was hidden")
        #expect(!model.anythingIsAgreed)
        #expect(!model.anyOfTheirWorkLeaves)
        #expect(model.summaryKey == "consent.summary.nothing_leaves")
    }

    @Test("crash reporting alone does not claim the user's work is being sent")
    func theDistinctionThatMatters() throws {
        // The whole reason the model carries two questions. A screen built on
        // "does anything leave the device" alone tells a user their recordings
        // are being uploaded when they are not.
        let policy = try Policy()
        try policy.grant(.crashDiagnostics, agreementVersion: 1)

        let model = try ConsentModel(policy: policy)
        #expect(model.anythingIsAgreed)
        #expect(!model.anyOfTheirWorkLeaves, "a crash report was reported as the user's work")
        #expect(model.summaryKey == "consent.summary.only_facts_leave")
    }

    @Test("agreeing to something that sends work says so")
    func workLeaving() throws {
        let policy = try Policy()
        try policy.grant(.projectSync, agreementVersion: 1)

        let model = try ConsentModel(policy: policy)
        #expect(model.anyOfTheirWorkLeaves)
        #expect(model.summaryKey == "consent.summary.work_leaves")
    }

    @Test("every purpose is listed whether or not it is on")
    func nothingIsHidden() throws {
        // Somebody opening this screen is usually asking what the application
        // *could* do, and a screen that hid what they had not agreed to would
        // make that impossible to find out.
        let model = try ConsentModel(policy: Policy())
        for purpose in Purpose.allCases {
            #expect(model.rows.contains { $0.purpose == purpose })
        }
        #expect(Set(model.rows.map(\.id)).count == model.rows.count, "two rows share an id")
        #expect(model.rows.allSatisfy { !$0.effectKey.isEmpty })
    }

    @Test("withdrawing puts the summary back")
    func withdrawing() throws {
        let policy = try Policy()
        try policy.grant(.projectSync, agreementVersion: 1)
        try policy.withdrawAll()

        let model = try ConsentModel(policy: policy)
        #expect(model.summaryKey == "consent.summary.nothing_leaves")
    }

    // MARK: - Synchronisation

    @Test("offline is a state, not an error, and editing works in it")
    func offlineIsNormal() throws {
        let sync = try Sync()
        let model = SyncModel(snapshot: try sync.snapshot())

        #expect(model.statusKey == "sync.offline")
        #expect(model.canEdit)
        #expect(!model.needsTheUser)
        #expect(!model.isBusy)
        #expect(model.badgeKey == nil, "an untouched installation was badged")
    }

    @Test("work done offline is counted, and the count is not a warning")
    func waitingWork() throws {
        let sync = try Sync()
        for sequence in 1...12 {
            try sync.hold(device: 1, sequence: UInt64(sequence))
        }

        let model = SyncModel(snapshot: try sync.snapshot())
        #expect(model.waiting == 12)
        #expect(!model.shouldWarnAboutBacklog)
        #expect(model.badgeKey == "sync.badge.waiting")
        #expect(model.canEdit)
    }

    @Test("a conflict asks the user and stops nothing else")
    func aConflictInterrupts() throws {
        let sync = try Sync()
        try sync.apply(.networkAvailable)
        try sync.apply(.workToSend)
        #expect(try sync.snapshot().isTransferring)

        try sync.apply(.conflictFound)
        let model = SyncModel(snapshot: try sync.snapshot())

        #expect(model.needsTheUser)
        #expect(!model.isBusy)
        #expect(model.canEdit, "an unanswered question stopped the user working")
        #expect(model.badgeKey == "sync.badge.needs_you")
    }

    @Test("a pause is lifted by the user and by nothing else")
    func aPauseHolds() throws {
        let sync = try Sync()
        try sync.apply(.pause)
        #expect(try sync.state() == .paused)

        for event: SyncEvent in [.networkAvailable, .workToSend, .workArrived, .transferFinished] {
            try sync.apply(event)
            #expect(try sync.state() == .paused, "a network event lifted a pause")
        }

        try sync.apply(.resume)
        #expect(try sync.state() != .paused)
    }

    @Test("work from a newer version is announced rather than silently missing")
    func newerVersion() throws {
        let sync = try Sync()
        let model = SyncModel(snapshot: try sync.snapshot(), carried: 3)

        #expect(model.needsANewerVersion)
        #expect(model.badgeKey == "sync.badge.newer_version")
    }

    @Test("one badge at a time, most pressing first")
    func badgesDoNotStack() throws {
        // A status area that stacks four notices is a status area people stop
        // reading.
        let sync = try Sync()
        try sync.apply(.networkAvailable)
        try sync.apply(.workToSend)
        try sync.apply(.conflictFound)
        try sync.hold(device: 1, sequence: 1)

        let model = SyncModel(snapshot: try sync.snapshot(), carried: 5)
        #expect(model.badgeKey == "sync.badge.needs_you")
        // And the quieter facts are still readable underneath it.
        #expect(model.waiting == 1)
        #expect(model.needsANewerVersion)
    }

    @Test("a snapshot is one consistent reading rather than several")
    func snapshotIsConsistent() throws {
        let sync = try Sync()
        try sync.apply(.networkAvailable)
        try sync.apply(.workArrived)
        try sync.hold(device: 1, sequence: 1)

        let snapshot = try sync.snapshot()
        #expect(snapshot.state == .receiving)
        #expect(snapshot.isTransferring)
        #expect(snapshot.waiting == 1)
        #expect(snapshot.editingIsAllowed)
    }

    @Test("every state and event has a key of its own")
    func vocabularyIsDistinct() throws {
        let states: [SyncState] = [.offline, .idle, .sending, .receiving, .conflicted, .paused]
        #expect(Set(states.map(\.key)).count == states.count)
        #expect(states.allSatisfy { $0.key.hasPrefix("sync.") })

        // And a state this wrapper does not know is representable rather than
        // crashing, with a key that names the number so a report can quote it.
        #expect(SyncState(code: 99) == .unrecognised(code: 99))
        #expect(SyncState(code: 99).key == "sync.unrecognised.99")
    }
}

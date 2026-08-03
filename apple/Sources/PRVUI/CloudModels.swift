import Foundation
import PRVCore

/// One line of the consent screen.
///
/// # Two questions, not one
///
/// A purpose can leave the device without carrying anything the user made — a
/// crash report is the plain example — and a screen that collapses those into
/// one sentence misleads in whichever direction it collapses them.
///
/// Built on "does anything leave the device" alone, it tells a user their
/// recordings are being sent when they are not. Built on "does this send
/// content" alone, it hides an upload entirely. So both travel, and the summary
/// below says both.
public struct ConsentRow: Sendable, Equatable, Identifiable {
    public let purpose: Purpose
    /// Whether the user has agreed to it.
    public let isGranted: Bool
    /// Whether agreeing means the user's own material is transmitted.
    public let sendsContent: Bool

    public var id: String { purpose.titleKey }

    /// The label a user reads, as a key rather than a sentence.
    public var titleKey: String { purpose.titleKey }

    /// What granting this would actually do, as a key.
    ///
    /// Three different sentences, because there are three different situations
    /// and a screen that used one for all of them would be wrong twice.
    public var effectKey: String {
        sendsContent ? "consent.effect.sends_your_work" : "consent.effect.sends_facts"
    }
}

/// The consent screen.
///
/// Holds no decisions of its own. Whether a purpose is granted, and what
/// granting it means, are the core's answers; this arranges them and works out
/// what the summary line should say.
public struct ConsentModel: Sendable, Equatable {
    public let rows: [ConsentRow]

    /// Builds the screen from what the core knows.
    ///
    /// Every purpose is listed, granted or not. A screen that hid the ones a
    /// user had not agreed to would make it impossible to find out what the
    /// application *could* do, which is the question somebody opening this
    /// screen is usually asking.
    public init(policy: Policy) throws {
        rows = try Purpose.allCases.map { purpose in
            ConsentRow(
                purpose: purpose,
                isGranted: try policy.allows(purpose),
                sendsContent: purpose.sendsContent
            )
        }
    }

    /// Builds one directly, for tests and previews.
    public init(rows: [ConsentRow]) {
        self.rows = rows
    }

    /// The purposes currently agreed to.
    public var granted: [ConsentRow] { rows.filter(\.isGranted) }

    /// Whether anything at all is agreed to.
    public var anythingIsAgreed: Bool { !granted.isEmpty }

    /// Whether anything the user made is transmitted.
    ///
    /// Deliberately narrower than "anything is agreed to". This is the sentence
    /// that matters most to somebody who came to this screen worried, and it
    /// must not be true merely because crash reporting is on.
    public var anyOfTheirWorkLeaves: Bool { granted.contains { $0.sendsContent } }

    /// The headline, as a key.
    public var summaryKey: String {
        if !anythingIsAgreed { return "consent.summary.nothing_leaves" }
        return anyOfTheirWorkLeaves
            ? "consent.summary.work_leaves"
            : "consent.summary.only_facts_leave"
    }
}

/// The synchronisation status a user sees.
///
/// # What it is careful about
///
/// Offline is not an error and is not shown as one. The state exists, it is
/// normal, editing works, and the only thing worth saying about it is that some
/// work has not travelled yet — which is a count, not a warning.
///
/// The one thing that *is* a warning arrives long before anything breaks: an
/// outbox close to its bound means a session has been offline for a very long
/// time or a server has been refusing everything, and a user who is told at the
/// refusal is told too late.
public struct SyncModel: Sendable, Equatable {
    public let snapshot: SyncSnapshot
    /// How many operations in this project were made by a newer build.
    public let carried: UInt64

    public init(snapshot: SyncSnapshot, carried: UInt64 = 0) {
        self.snapshot = snapshot
        self.carried = carried
    }

    /// The status line, as a key.
    public var statusKey: String { snapshot.state.key }

    /// Whether the user is being asked to decide something.
    public var needsTheUser: Bool { snapshot.needsTheUser }

    /// Whether editing is available. Always true, and shown to be.
    public var canEdit: Bool { snapshot.editingIsAllowed }

    /// Whether to show progress rather than a resting state.
    public var isBusy: Bool { snapshot.isTransferring }

    /// What has been done here and gone nowhere yet.
    public var waiting: UInt64 { snapshot.waiting }

    /// Whether to warn about the outbox.
    ///
    /// Before the refusal, not after it.
    public var shouldWarnAboutBacklog: Bool { snapshot.isNearlyFull }

    /// Whether part of this project was made with a newer version.
    ///
    /// Worth saying out loud. Those operations are kept and passed on, and they
    /// cannot be shown — so without a word about it the project silently appears
    /// to be missing work.
    public var needsANewerVersion: Bool { carried > 0 }

    /// The badge, as a key, or `nil` when there is nothing worth interrupting
    /// for.
    ///
    /// One badge at a time, most pressing first. A status area that stacks four
    /// notices is a status area people stop reading.
    public var badgeKey: String? {
        if needsTheUser { return "sync.badge.needs_you" }
        if shouldWarnAboutBacklog { return "sync.badge.backlog" }
        if needsANewerVersion { return "sync.badge.newer_version" }
        if waiting > 0 { return "sync.badge.waiting" }
        return nil
    }
}

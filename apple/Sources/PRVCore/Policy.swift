import CPRVBridge
import Foundation

/// Something the user may agree to.
public enum Purpose: Sendable, CaseIterable {
    case cloudAnalysis
    case cloudLanguage
    case cloudStemSeparation
    case projectSync
    case collaboration
    case crashDiagnostics
    case usageAnalytics
    /// Using the user's projects to improve the models.
    ///
    /// Listed like any other purpose and, like any other, off until asked for.
    case modelTraining
    case personalisedSuggestions

    var code: Int32 {
        switch self {
        case .cloudAnalysis: PRV_PURPOSE_CLOUD_ANALYSIS.rawValue
        case .cloudLanguage: PRV_PURPOSE_CLOUD_LANGUAGE.rawValue
        case .cloudStemSeparation: PRV_PURPOSE_CLOUD_STEM_SEPARATION.rawValue
        case .projectSync: PRV_PURPOSE_PROJECT_SYNC.rawValue
        case .collaboration: PRV_PURPOSE_COLLABORATION.rawValue
        case .crashDiagnostics: PRV_PURPOSE_CRASH_DIAGNOSTICS.rawValue
        case .usageAnalytics: PRV_PURPOSE_USAGE_ANALYTICS.rawValue
        case .modelTraining: PRV_PURPOSE_MODEL_TRAINING.rawValue
        case .personalisedSuggestions: PRV_PURPOSE_PERSONALISED_SUGGESTIONS.rawValue
        }
    }

    /// A stable key for the label a user reads.
    public var titleKey: String { "purpose.\(self)" }

    /// Whether this sends the user's own material rather than a fact about it.
    ///
    /// # A consent screen needs this *and* the headline
    ///
    /// "We send the tempo we measured" and "we send the recording" are both
    /// cloud processing, and a user told only the first is being misled. This is
    /// the second question; ``Policy/anythingLeavesTheDevice`` is whether
    /// anything is transmitted at all — a crash report leaves and carries no
    /// music, so the two genuinely differ.
    public var sendsContent: Bool {
        var value: Int32 = 0
        guard prv_purpose_sends_content(code, &value) == PRV_OK.rawValue else { return false }
        return value != 0
    }
}

/// What a licence costs and unlocks.
public enum Tier: Sendable, CaseIterable, Comparable {
    case free
    case standard
    case professional
    case studio

    var code: Int32 {
        switch self {
        case .free: PRV_TIER_FREE.rawValue
        case .standard: PRV_TIER_STANDARD.rawValue
        case .professional: PRV_TIER_PROFESSIONAL.rawValue
        case .studio: PRV_TIER_STUDIO.rawValue
        }
    }

    init?(code: Int32) {
        guard let match = Tier.allCases.first(where: { $0.code == code }) else { return nil }
        self = match
    }
}

/// What the user agreed to, and what their licence allows.
///
/// # The order these are asked in is the design
///
/// *May this leave the device* comes before *is this feature available*. A user
/// who has not agreed to cloud analysis is not shown a paywall for it — they are
/// simply not sent anywhere, whatever tier they are on. Keeping both on one type
/// is what makes that order visible instead of a convention a host can invert.
public final class Policy {
    private let handle: OpaquePointer

    /// Nothing agreed to, free tier.
    public init() throws {
        var created: OpaquePointer?
        try EngineError.check(prv_policy_create(&created))
        guard let created else { throw EngineError.invalidHandle }
        handle = created
    }

    deinit {
        prv_policy_destroy(handle)
    }

    /// Records that the user agreed to a purpose.
    ///
    /// - Parameter agreementVersion: which wording they agreed to. Kept by the
    ///   core so an audit can say *which* agreement was given, which is the
    ///   difference between a consent record and a boolean.
    public func grant(_ purpose: Purpose, agreementVersion: UInt64) throws {
        try EngineError.check(prv_policy_grant(handle, purpose.code, agreementVersion))
    }

    /// Records that the user withdrew a purpose.
    public func withdraw(_ purpose: Purpose) throws {
        try EngineError.check(prv_policy_withdraw(handle, purpose.code))
    }

    /// Withdraws everything at once.
    public func withdrawAll() throws {
        try EngineError.check(prv_policy_withdraw_all(handle))
    }

    /// Whether a purpose is currently agreed to.
    public func allows(_ purpose: Purpose) throws -> Bool {
        var value: Int32 = 0
        try EngineError.check(prv_policy_allows(handle, purpose.code, &value))
        return value != 0
    }

    /// Whether anything at all currently leaves the device.
    ///
    /// The single question a privacy screen leads with, and the one a user
    /// checks before a set in a venue with no network worth trusting.
    public var anythingLeavesTheDevice: Bool {
        var value: Int32 = 0
        guard
            prv_policy_anything_leaves_the_device(handle, &value) == PRV_OK.rawValue
        else { return true }  // Unknown is not "safe"; say yes and be checked.
        return value != 0
    }

    /// The current licence tier.
    public var tier: Tier {
        var value: Int32 = 0
        guard prv_policy_tier(handle, &value) == PRV_OK.rawValue else { return .free }
        return Tier(code: value) ?? .free
    }

    /// Sets the licence tier.
    public func setTier(_ tier: Tier) throws {
        try EngineError.check(prv_policy_set_tier(handle, tier.code))
    }

    /// Marks the licence expired. Everything essential survives.
    public func expire() throws {
        try EngineError.check(prv_policy_expire(handle))
    }

    /// Whether a feature is available, and whether it is essential.
    ///
    /// Returned together because a host needs both to decide what to *show*: an
    /// unavailable non-essential feature is an upgrade prompt, and an
    /// unavailable essential one is a bug report.
    public func availability(ofFeature index: Int32) throws -> (allowed: Bool, essential: Bool) {
        var allowed: Int32 = 0
        var essential: Int32 = 0
        try EngineError.check(prv_policy_feature_allowed(handle, index, &allowed))
        try EngineError.check(prv_policy_feature_is_essential(handle, index, &essential))
        return (allowed != 0, essential != 0)
    }
}

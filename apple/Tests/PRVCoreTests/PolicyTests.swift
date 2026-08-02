import Foundation
import Testing

@testable import PRVCore

@Suite("Consent and licensing")
struct PolicyTests {

    @Test("a new policy sends nothing anywhere")
    func nothingByDefault() throws {
        // Master Prompt #26. A host that forgets to ask must not become a host
        // that uploads somebody's music.
        let policy = try Policy()
        #expect(!policy.anythingLeavesTheDevice)
        for purpose in Purpose.allCases {
            #expect(try !policy.allows(purpose), "\(purpose) was on by default")
        }
        #expect(policy.tier == .free)
    }

    @Test("granting and withdrawing round-trips, and withdrawing all clears it")
    func grantAndWithdraw() throws {
        let policy = try Policy()
        try policy.grant(.projectSync, agreementVersion: 3)
        #expect(try policy.allows(.projectSync))
        #expect(policy.anythingLeavesTheDevice)

        try policy.withdraw(.projectSync)
        #expect(try !policy.allows(.projectSync))
        #expect(!policy.anythingLeavesTheDevice)

        for purpose in Purpose.allCases {
            try policy.grant(purpose, agreementVersion: 1)
        }
        try policy.withdrawAll()
        for purpose in Purpose.allCases {
            #expect(try !policy.allows(purpose), "\(purpose) survived withdrawing everything")
        }
    }

    @Test("sending content and leaving the device are different questions")
    func twoDistinctQuestions() throws {
        // The distinction a consent screen has to make honestly. Every purpose
        // that sends the user's material also leaves the device; not every
        // purpose that leaves the device sends their material — a crash report
        // does one and not the other.
        for purpose in Purpose.allCases where purpose.sendsContent {
            let policy = try Policy()
            try policy.grant(purpose, agreementVersion: 1)
            #expect(
                policy.anythingLeavesTheDevice,
                "\(purpose) sends material without leaving the device"
            )
        }

        #expect(Purpose.cloudAnalysis.sendsContent)
        #expect(!Purpose.crashDiagnostics.sendsContent)
        #expect(
            Purpose.allCases.contains { !$0.sendsContent },
            "the distinction would be theoretical if every purpose sent content"
        )
    }

    @Test("using projects to train models is a purpose like any other, and off")
    func trainingIsOptIn() throws {
        // A named, standing constraint: never use a user's projects for training
        // without explicit permission. Asserted here because this is the surface
        // an application actually asks.
        let policy = try Policy()
        #expect(try !policy.allows(.modelTraining))
        #expect(Purpose.modelTraining.sendsContent, "training on projects sends the projects")
    }

    @Test("a purchase does not grant consent, and consent does not buy a tier")
    func independence() throws {
        let paid = try Policy()
        try paid.setTier(.studio)
        #expect(!paid.anythingLeavesTheDevice, "a purchase granted consent")

        let agreeable = try Policy()
        for purpose in Purpose.allCases {
            try agreeable.grant(purpose, agreementVersion: 1)
        }
        #expect(agreeable.tier == .free, "consent bought a tier")
    }

    @Test("an expired licence keeps everything essential")
    func expiryIsNotLockout() throws {
        // A user whose card failed still gets their work back.
        let policy = try Policy()
        try policy.setTier(.studio)
        try policy.expire()
        #expect(policy.tier == .free)

        for index in Int32(0)..<Int32(13) {
            let (allowed, essential) = try policy.availability(ofFeature: index)
            if essential {
                #expect(allowed, "feature \(index) is essential and was lost on expiry")
            }
        }
    }

    @Test("a higher tier never takes a feature away")
    func tiersOnlyAdd() throws {
        let ordered = Tier.allCases
        for (lower, higher) in zip(ordered, ordered.dropFirst()) {
            let below = try Policy()
            try below.setTier(lower)
            let above = try Policy()
            try above.setTier(higher)

            for index in Int32(0)..<Int32(13) {
                if try below.availability(ofFeature: index).allowed {
                    #expect(
                        try above.availability(ofFeature: index).allowed,
                        "\(higher) lost a feature \(lower) had"
                    )
                }
            }
        }
    }

    @Test("an unknown feature index is refused rather than reported unavailable")
    func unknownFeature() throws {
        // Reporting it unavailable would make a typo look like a paywall.
        let policy = try Policy()
        #expect(throws: EngineError.invalidArgument) {
            _ = try policy.availability(ofFeature: 9_999)
        }
    }
}

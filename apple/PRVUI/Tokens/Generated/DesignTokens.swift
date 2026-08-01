// GENERATED FILE — DO NOT EDIT.
//
// Produced by tools/tokengen from design/tokens/tokens.json.
// Edit the token source and regenerate; hand edits fail continuous integration.
//
// Master Prompt #16: the design token system is the single source of truth for
// every visual value. No screen may bypass it, and no visual value may be
// hardcoded downstream of this file.
//
// Deliberately free of framework imports so that it compiles on every platform
// the core targets. The mapping to Color, Font and Animation is a hand-written
// adapter in the presentation layer.

public enum DesignTokens {
    /// Version of the token source this file was generated from.
    public static let version: Int = 1
}

/// A colour with an appearance for each of light and dark.
///
/// Both appearances are always present. Master Prompt #8 requires light, dark
/// and high contrast from day one, and a colour that exists in only one
/// appearance is a colour that will be wrong in the other.
public struct ColorToken: Sendable, Equatable {
    /// Hex string, `#RRGGBB` or `#RRGGBBAA`.
    public let light: String
    /// Hex string, `#RRGGBB` or `#RRGGBBAA`.
    public let dark: String

    public init(light: String, dark: String) {
        self.light = light
        self.dark = dark
    }
}

public enum ColorTokens {
    /// The product accent, used when no artwork-derived accent is available. Master Prompt #2 requires the accent to follow artwork where one exists; this is the fallback and the brand anchor.
    public static let accentPrimary = ColorToken(light: "#4A3AFF", dark: "#6F62FF")

    /// Intelligence running on an external service. Paired with an explicit label wherever it appears.
    public static let aiCloud = ColorToken(light: "#7A3E9D", dark: "#B57BE0")

    /// Intelligence running on the device. Master Prompt #26 requires the user to be able to tell at a glance where their data is being processed, so on-device and cloud processing are visually distinct states rather than one undifferentiated 'AI' colour.
    public static let aiOnDevice = ColorToken(light: "#1B6E8C", dark: "#48B7DB")

    /// Titles and body text.
    public static let contentPrimary = ColorToken(light: "#0A0A0B", dark: "#F5F5F7")

    /// Supporting text and metadata.
    public static let contentSecondary = ColorToken(light: "#5B5B63", dark: "#A0A0AA")

    /// Placeholders and disabled labels.
    public static let contentTertiary = ColorToken(light: "#8E8E98", dark: "#6E6E78")

    /// True peak at or above the ceiling. Always accompanied by a persistent clip indicator, never colour alone.
    public static let meterClipping = ColorToken(light: "#B3261E", dark: "#FF6259")

    /// Level metering approaching the ceiling.
    public static let meterHot = ColorToken(light: "#9A6200", dark: "#E0A106")

    /// Level metering below the headroom threshold.
    public static let meterNominal = ColorToken(light: "#1B7F4B", dark: "#35C77B")

    /// Hairline division between regions.
    public static let separator = ColorToken(light: "#0000001F", dark: "#FFFFFF1A")

    /// Clipping, data loss risk, failed operation.
    public static let statusDanger = ColorToken(light: "#B3261E", dark: "#FF6259")

    /// Completed export, successful sync.
    public static let statusSuccess = ColorToken(light: "#1B7F4B", dark: "#35C77B")

    /// Recoverable problem needing attention.
    public static let statusWarning = ColorToken(light: "#9A6200", dark: "#E0A106")

    /// The furthest-back surface. True black in dark mode so that glass layers above it have something to lift away from.
    public static let surfaceCanvas = ColorToken(light: "#F7F7F9", dark: "#000000")

    /// Translucent material for floating panels and the inspector. Alpha is part of the token because a glass surface without its alpha is not the same material.
    public static let surfaceGlass = ColorToken(light: "#FFFFFFB8", dark: "#1C1C21A6")

    /// Scrim behind modal content.
    public static let surfaceOverlay = ColorToken(light: "#00000052", dark: "#000000A3")

    /// Cards and panels resting on the canvas.
    public static let surfaceRaised = ColorToken(light: "#FFFFFF", dark: "#111114")

    /// High-frequency energy.
    public static let waveformHigh = ColorToken(light: "#C8791B", dark: "#F0A63E")

    /// Low-frequency energy in the frequency-coloured waveform.
    public static let waveformLow = ColorToken(light: "#2F6FED", dark: "#5C93FF")

    /// Mid-frequency energy.
    public static let waveformMid = ColorToken(light: "#1B9E6E", dark: "#39C795")

    /// Every colour token, keyed by its source name.
    ///
    /// Used by the token inspector in developer mode and by snapshot tests that
    /// assert every token renders in both appearances.
    public static let all: [String: ColorToken] = [
        "accent.primary": accentPrimary,
        "ai.cloud": aiCloud,
        "ai.onDevice": aiOnDevice,
        "content.primary": contentPrimary,
        "content.secondary": contentSecondary,
        "content.tertiary": contentTertiary,
        "meter.clipping": meterClipping,
        "meter.hot": meterHot,
        "meter.nominal": meterNominal,
        "separator": separator,
        "status.danger": statusDanger,
        "status.success": statusSuccess,
        "status.warning": statusWarning,
        "surface.canvas": surfaceCanvas,
        "surface.glass": surfaceGlass,
        "surface.overlay": surfaceOverlay,
        "surface.raised": surfaceRaised,
        "waveform.high": waveformHigh,
        "waveform.low": waveformLow,
        "waveform.mid": waveformMid,
    ]
}

/// Layout spacing scale, in points.
public enum Spacing {
    public static let huge: Double = 64
    public static let lg: Double = 16
    public static let md: Double = 12
    public static let sm: Double = 8
    public static let xl: Double = 24
    public static let xs: Double = 4
    public static let xxl: Double = 32
    public static let xxs: Double = 2
    public static let xxxl: Double = 48
}

/// Corner radius scale, in points.
public enum Radius {
    public static let lg: Double = 18
    public static let md: Double = 12
    public static let none: Double = 0
    public static let pill: Double = 999
    public static let sm: Double = 6
    public static let xl: Double = 26
    public static let xxl: Double = 34
}

/// Named opacities.
public enum Opacity {
    public static let disabled: Double = 0.38
    public static let full: Double = 1
    public static let hover: Double = 0.08
    public static let pressed: Double = 0.16
    public static let secondary: Double = 0.6
}

/// Material blur radii, in points.
public enum Blur {
    public static let heavy: Double = 40
    public static let regular: Double = 20
    public static let subtle: Double = 8
}

/// A text style.
///
/// `textStyle` names the platform text style the size scales with, so that
/// Dynamic Type is inherited rather than reimplemented. `monospacedDigits`
/// matters wherever a number changes while the user is reading it: without
/// tabular figures, a BPM readout shifts horizontally on every update.
public struct TypographyToken: Sendable, Equatable {
    public let size: Double
    public let weight: String
    public let textStyle: String
    public let tracking: Double
    public let monospacedDigits: Bool

    public init(size: Double, weight: String, textStyle: String, tracking: Double, monospacedDigits: Bool) {
        self.size = size
        self.weight = weight
        self.textStyle = textStyle
        self.tracking = tracking
        self.monospacedDigits = monospacedDigits
    }
}

public enum Typography {
    public static let body = TypographyToken(size: 17, weight: "regular", textStyle: "body", tracking: 0, monospacedDigits: false)
    public static let callout = TypographyToken(size: 16, weight: "regular", textStyle: "callout", tracking: 0, monospacedDigits: false)
    public static let caption = TypographyToken(size: 12, weight: "regular", textStyle: "caption", tracking: 0.1, monospacedDigits: false)
    public static let display = TypographyToken(size: 44, weight: "bold", textStyle: "largeTitle", tracking: -0.5, monospacedDigits: false)
    public static let headline = TypographyToken(size: 17, weight: "semibold", textStyle: "headline", tracking: 0, monospacedDigits: false)
    public static let largeTitle = TypographyToken(size: 34, weight: "bold", textStyle: "largeTitle", tracking: -0.4, monospacedDigits: false)
    public static let mono = TypographyToken(size: 13, weight: "regular", textStyle: "footnote", tracking: 0, monospacedDigits: true)
    public static let numeric = TypographyToken(size: 15, weight: "medium", textStyle: "body", tracking: 0, monospacedDigits: true)
    public static let title = TypographyToken(size: 28, weight: "semibold", textStyle: "title", tracking: -0.3, monospacedDigits: false)
}

/// A motion token: how long a change takes and how it travels.
///
/// Master Prompt #16 requires motion to communicate state rather than decorate,
/// so these are named by intent. Every one collapses to zero duration when the
/// system requests reduced motion: the state change still happens, it simply
/// arrives without travel.
public struct MotionToken: Sendable, Equatable {
    /// Duration in milliseconds.
    public let duration: Double
    /// Name of the curve in `MotionCurves`.
    public let curve: String

    public init(duration: Double, curve: String) {
        self.duration = duration
        self.curve = curve
    }

    /// The same token with travel removed, for reduced-motion contexts.
    public var reducedMotion: MotionToken {
        MotionToken(duration: 0, curve: "linear")
    }
}

public enum Motion {
    /// No animation. What every other token becomes under reduced motion.
    public static let instant = MotionToken(duration: 0, curve: "linear")

    /// Timeline construction as a mix is generated, where the motion is the explanation.
    public static let long = MotionToken(duration: 520, curve: "easeInOut")

    /// Navigation between spaces, panel presentation.
    public static let medium = MotionToken(duration: 340, curve: "spring")

    /// Press feedback, selection, toggle.
    public static let micro = MotionToken(duration: 120, curve: "easeOut")

    /// Appear, disappear, expand, collapse.
    public static let short = MotionToken(duration: 220, curve: "spring")

}

/// A shadow definition.
public struct ElevationToken: Sendable, Equatable {
    public let y: Double
    public let blur: Double
    public let opacity: Double

    public init(y: Double, blur: Double, opacity: Double) {
        self.y = y
        self.blur = blur
        self.opacity = opacity
    }
}

public enum Elevation {
    public static let flat = ElevationToken(y: 0, blur: 0, opacity: 0)
    public static let floating = ElevationToken(y: 6, blur: 18, opacity: 0.18)
    public static let modal = ElevationToken(y: 16, blur: 44, opacity: 0.26)
    public static let raised = ElevationToken(y: 1, blur: 3, opacity: 0.12)
}

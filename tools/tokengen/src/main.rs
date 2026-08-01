//! Generates platform bindings from `design/tokens/tokens.json`.
//!
//! # Why generate rather than hand-write
//!
//! Master Prompt #16 states the rule plainly: never hardcode visual values, and
//! no screen may bypass the token system. A hand-maintained Swift copy of the
//! tokens is a second source of truth, and two sources of truth diverge — not
//! immediately, but by the third release, quietly, in one appearance nobody
//! tested.
//!
//! Generating removes the possibility. The generated file is checked by
//! continuous integration against a fresh run, so a hand edit fails the build.
//!
//! # What is generated
//!
//! Plain Swift with no framework imports: structures and constants only. The
//! mapping from these values to `Color`, `Font` and `Animation` is a small
//! hand-written adapter in the presentation layer.
//!
//! Keeping the generated half framework-free has a practical payoff beyond
//! tidiness — it compiles anywhere Swift compiles, including the Linux runners
//! that gate every pull request, so token changes are verified without needing
//! scarce macOS capacity.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde_json::Value;

/// Path of the token source, relative to the repository root.
const SOURCE: &str = "design/tokens/tokens.json";

/// Path of the generated Swift file, relative to the repository root.
const SWIFT_OUTPUT: &str = "apple/PRVUI/Tokens/Generated/DesignTokens.swift";

fn main() -> ExitCode {
    let root = match repository_root() {
        Some(root) => root,
        None => {
            eprintln!("could not locate the repository root");
            return ExitCode::FAILURE;
        }
    };

    let check_only = std::env::args().any(|argument| argument == "--check");

    match run(&root, check_only) {
        Ok(Outcome::Written(path)) => {
            println!("generated {}", path.display());
            ExitCode::SUCCESS
        }
        Ok(Outcome::UpToDate) => {
            println!("generated bindings are up to date");
            ExitCode::SUCCESS
        }
        Ok(Outcome::Stale) => {
            eprintln!(
                "generated bindings are out of date.\n\
                 Run `cargo run --manifest-path tools/tokengen/Cargo.toml` and commit the result.\n\
                 Generated files are never edited by hand (Master Prompt #16)."
            );
            ExitCode::FAILURE
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

/// What a run did.
enum Outcome {
    Written(PathBuf),
    UpToDate,
    Stale,
}

/// Reads the token source and writes or verifies the generated bindings.
fn run(root: &Path, check_only: bool) -> Result<Outcome, String> {
    let source_path = root.join(SOURCE);
    let raw = fs::read_to_string(&source_path)
        .map_err(|error| format!("cannot read {}: {error}", source_path.display()))?;
    let tokens: Value = serde_json::from_str(&raw)
        .map_err(|error| format!("{} is not valid JSON: {error}", source_path.display()))?;

    let swift = generate_swift(&tokens)?;
    let output_path = root.join(SWIFT_OUTPUT);

    if check_only {
        let existing = fs::read_to_string(&output_path).unwrap_or_default();
        return Ok(if existing == swift {
            Outcome::UpToDate
        } else {
            Outcome::Stale
        });
    }

    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    fs::write(&output_path, swift)
        .map_err(|error| format!("cannot write {}: {error}", output_path.display()))?;
    Ok(Outcome::Written(output_path))
}

/// Walks up from the executable's working directory to find the repository root.
fn repository_root() -> Option<PathBuf> {
    let mut directory = std::env::current_dir().ok()?;
    loop {
        if directory.join(SOURCE).is_file() {
            return Some(directory);
        }
        if !directory.pop() {
            return None;
        }
    }
}

/// Renders the Swift binding file.
fn generate_swift(tokens: &Value) -> Result<String, String> {
    let version = tokens
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| "token source has no numeric `version`".to_string())?;

    let mut out = String::new();
    out.push_str(&header());
    out.push_str(&format!(
        "public enum DesignTokens {{\n    /// Version of the token source this file was generated from.\n    public static let version: Int = {version}\n}}\n\n"
    ));

    out.push_str(&color_section(tokens)?);
    out.push_str(&scalar_section(
        tokens,
        "spacing",
        "Spacing",
        "Layout spacing scale, in points.",
    )?);
    out.push_str(&scalar_section(
        tokens,
        "radius",
        "Radius",
        "Corner radius scale, in points.",
    )?);
    out.push_str(&scalar_section(
        tokens,
        "opacity",
        "Opacity",
        "Named opacities.",
    )?);
    out.push_str(&scalar_section(
        tokens,
        "blur",
        "Blur",
        "Material blur radii, in points.",
    )?);
    out.push_str(&typography_section(tokens)?);
    out.push_str(&motion_section(tokens)?);
    out.push_str(&elevation_section(tokens)?);

    Ok(out)
}

/// The banner every generated file carries.
fn header() -> String {
    "// GENERATED FILE — DO NOT EDIT.\n\
     //\n\
     // Produced by tools/tokengen from design/tokens/tokens.json.\n\
     // Edit the token source and regenerate; hand edits fail continuous integration.\n\
     //\n\
     // Master Prompt #16: the design token system is the single source of truth for\n\
     // every visual value. No screen may bypass it, and no visual value may be\n\
     // hardcoded downstream of this file.\n\
     //\n\
     // Deliberately free of framework imports so that it compiles on every platform\n\
     // the core targets. The mapping to Color, Font and Animation is a hand-written\n\
     // adapter in the presentation layer.\n\n"
        .to_string()
}

/// Emits the colour tokens.
fn color_section(tokens: &Value) -> Result<String, String> {
    let entries = tokens
        .pointer("/color/tokens")
        .and_then(Value::as_object)
        .ok_or_else(|| "token source has no `color.tokens` object".to_string())?;

    let mut out = String::new();
    out.push_str(
        "/// A colour with an appearance for each of light and dark.\n\
         ///\n\
         /// Both appearances are always present. Master Prompt #8 requires light, dark\n\
         /// and high contrast from day one, and a colour that exists in only one\n\
         /// appearance is a colour that will be wrong in the other.\n\
         public struct ColorToken: Sendable, Equatable {\n\
         \x20   /// Hex string, `#RRGGBB` or `#RRGGBBAA`.\n\
         \x20   public let light: String\n\
         \x20   /// Hex string, `#RRGGBB` or `#RRGGBBAA`.\n\
         \x20   public let dark: String\n\n\
         \x20   public init(light: String, dark: String) {\n\
         \x20       self.light = light\n\
         \x20       self.dark = dark\n\
         \x20   }\n\
         }\n\n\
         public enum ColorTokens {\n",
    );

    let mut names = Vec::new();
    for (key, value) in entries {
        let light = value
            .get("light")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("colour `{key}` has no `light` value"))?;
        let dark = value
            .get("dark")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("colour `{key}` has no `dark` value"))?;
        let identifier = to_camel_case(key);

        if let Some(description) = value.get("description").and_then(Value::as_str) {
            out.push_str(&doc_comment(description, 4));
        }
        out.push_str(&format!(
            "    public static let {identifier} = ColorToken(light: \"{light}\", dark: \"{dark}\")\n\n"
        ));
        names.push((key.clone(), identifier));
    }

    out.push_str(
        "    /// Every colour token, keyed by its source name.\n\
         \x20   ///\n\
         \x20   /// Used by the token inspector in developer mode and by snapshot tests that\n\
         \x20   /// assert every token renders in both appearances.\n\
         \x20   public static let all: [String: ColorToken] = [\n",
    );
    for (key, identifier) in &names {
        out.push_str(&format!("        \"{key}\": {identifier},\n"));
    }
    out.push_str("    ]\n}\n\n");

    Ok(out)
}

/// Emits a group of plain numeric tokens.
fn scalar_section(
    tokens: &Value,
    group: &str,
    type_name: &str,
    summary: &str,
) -> Result<String, String> {
    let entries = tokens
        .pointer(&format!("/{group}/tokens"))
        .and_then(Value::as_object)
        .ok_or_else(|| format!("token source has no `{group}.tokens` object"))?;

    let mut out = String::new();
    out.push_str(&doc_comment(summary, 0));
    out.push_str(&format!("public enum {type_name} {{\n"));
    for (key, value) in entries {
        let number = value
            .as_f64()
            .ok_or_else(|| format!("`{group}.{key}` is not a number"))?;
        out.push_str(&format!(
            "    public static let {}: Double = {}\n",
            to_camel_case(key),
            format_number(number)
        ));
    }
    out.push_str("}\n\n");
    Ok(out)
}

/// Emits the typography tokens.
fn typography_section(tokens: &Value) -> Result<String, String> {
    let entries = tokens
        .pointer("/typography/tokens")
        .and_then(Value::as_object)
        .ok_or_else(|| "token source has no `typography.tokens` object".to_string())?;

    let mut out = String::new();
    out.push_str(
        "/// A text style.\n\
         ///\n\
         /// `textStyle` names the platform text style the size scales with, so that\n\
         /// Dynamic Type is inherited rather than reimplemented. `monospacedDigits`\n\
         /// matters wherever a number changes while the user is reading it: without\n\
         /// tabular figures, a BPM readout shifts horizontally on every update.\n\
         public struct TypographyToken: Sendable, Equatable {\n\
         \x20   public let size: Double\n\
         \x20   public let weight: String\n\
         \x20   public let textStyle: String\n\
         \x20   public let tracking: Double\n\
         \x20   public let monospacedDigits: Bool\n\n\
         \x20   public init(size: Double, weight: String, textStyle: String, tracking: Double, monospacedDigits: Bool) {\n\
         \x20       self.size = size\n\
         \x20       self.weight = weight\n\
         \x20       self.textStyle = textStyle\n\
         \x20       self.tracking = tracking\n\
         \x20       self.monospacedDigits = monospacedDigits\n\
         \x20   }\n\
         }\n\n\
         public enum Typography {\n",
    );

    for (key, value) in entries {
        let size = value
            .get("size")
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("typography `{key}` has no `size`"))?;
        let weight = value
            .get("weight")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("typography `{key}` has no `weight`"))?;
        let text_style = value
            .get("textStyle")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("typography `{key}` has no `textStyle`"))?;
        let tracking = value.get("tracking").and_then(Value::as_f64).unwrap_or(0.0);
        let monospaced = value
            .get("monospacedDigits")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        out.push_str(&format!(
            "    public static let {} = TypographyToken(size: {}, weight: \"{weight}\", textStyle: \"{text_style}\", tracking: {}, monospacedDigits: {monospaced})\n",
            to_camel_case(key),
            format_number(size),
            format_number(tracking)
        ));
    }
    out.push_str("}\n\n");
    Ok(out)
}

/// Emits motion durations and curves.
fn motion_section(tokens: &Value) -> Result<String, String> {
    let entries = tokens
        .pointer("/motion/tokens")
        .and_then(Value::as_object)
        .ok_or_else(|| "token source has no `motion.tokens` object".to_string())?;

    let mut out = String::new();
    out.push_str(
        "/// A motion token: how long a change takes and how it travels.\n\
         ///\n\
         /// Master Prompt #16 requires motion to communicate state rather than decorate,\n\
         /// so these are named by intent. Every one collapses to zero duration when the\n\
         /// system requests reduced motion: the state change still happens, it simply\n\
         /// arrives without travel.\n\
         public struct MotionToken: Sendable, Equatable {\n\
         \x20   /// Duration in milliseconds.\n\
         \x20   public let duration: Double\n\
         \x20   /// Name of the curve in `MotionCurves`.\n\
         \x20   public let curve: String\n\n\
         \x20   public init(duration: Double, curve: String) {\n\
         \x20       self.duration = duration\n\
         \x20       self.curve = curve\n\
         \x20   }\n\n\
         \x20   /// The same token with travel removed, for reduced-motion contexts.\n\
         \x20   public var reducedMotion: MotionToken {\n\
         \x20       MotionToken(duration: 0, curve: \"linear\")\n\
         \x20   }\n\
         }\n\n\
         public enum Motion {\n",
    );

    for (key, value) in entries {
        let duration = value
            .get("duration")
            .and_then(Value::as_f64)
            .ok_or_else(|| format!("motion `{key}` has no `duration`"))?;
        let curve = value
            .get("curve")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("motion `{key}` has no `curve`"))?;
        if let Some(description) = value.get("description").and_then(Value::as_str) {
            out.push_str(&doc_comment(description, 4));
        }
        out.push_str(&format!(
            "    public static let {} = MotionToken(duration: {}, curve: \"{curve}\")\n\n",
            to_camel_case(key),
            format_number(duration)
        ));
    }
    out.push_str("}\n\n");
    Ok(out)
}

/// Emits shadow definitions.
fn elevation_section(tokens: &Value) -> Result<String, String> {
    let entries = tokens
        .pointer("/elevation/tokens")
        .and_then(Value::as_object)
        .ok_or_else(|| "token source has no `elevation.tokens` object".to_string())?;

    let mut out = String::new();
    out.push_str(
        "/// A shadow definition.\n\
         public struct ElevationToken: Sendable, Equatable {\n\
         \x20   public let y: Double\n\
         \x20   public let blur: Double\n\
         \x20   public let opacity: Double\n\n\
         \x20   public init(y: Double, blur: Double, opacity: Double) {\n\
         \x20       self.y = y\n\
         \x20       self.blur = blur\n\
         \x20       self.opacity = opacity\n\
         \x20   }\n\
         }\n\n\
         public enum Elevation {\n",
    );

    for (key, value) in entries {
        let y = value.get("y").and_then(Value::as_f64).unwrap_or(0.0);
        let blur = value.get("blur").and_then(Value::as_f64).unwrap_or(0.0);
        let opacity = value.get("opacity").and_then(Value::as_f64).unwrap_or(0.0);
        out.push_str(&format!(
            "    public static let {} = ElevationToken(y: {}, blur: {}, opacity: {})\n",
            to_camel_case(key),
            format_number(y),
            format_number(blur),
            format_number(opacity)
        ));
    }
    out.push_str("}\n");
    Ok(out)
}

/// Converts a dotted token name to a Swift identifier.
///
/// `surface.canvas` becomes `surfaceCanvas`, `ai.onDevice` becomes `aiOnDevice`.
fn to_camel_case(key: &str) -> String {
    let mut result = String::with_capacity(key.len());
    let mut capitalise_next = false;
    for character in key.chars() {
        if character == '.' || character == '-' || character == '_' {
            capitalise_next = true;
            continue;
        }
        if capitalise_next {
            result.extend(character.to_uppercase());
            capitalise_next = false;
        } else {
            result.push(character);
        }
    }
    result
}

/// Formats a number so that whole values render without a trailing decimal.
fn format_number(value: f64) -> String {
    if (value.fract()).abs() < f64::EPSILON {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

/// Wraps text as a Swift documentation comment at the given indent.
fn doc_comment(text: &str, indent: usize) -> String {
    let padding = " ".repeat(indent);
    text.split('\n')
        .map(|line| format!("{padding}/// {line}\n"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotted_names_become_camel_case_identifiers() {
        assert_eq!(to_camel_case("surface.canvas"), "surfaceCanvas");
        assert_eq!(to_camel_case("ai.onDevice"), "aiOnDevice");
        assert_eq!(to_camel_case("xxs"), "xxs");
        assert_eq!(to_camel_case("waveform.low"), "waveformLow");
    }

    #[test]
    fn whole_numbers_render_without_a_decimal_point() {
        assert_eq!(format_number(8.0), "8");
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(0.38), "0.38");
        assert_eq!(format_number(-0.5), "-0.5");
    }

    #[test]
    fn generation_fails_loudly_on_a_malformed_source() {
        let malformed: Value = serde_json::json!({ "version": 1 });
        let result = generate_swift(&malformed);
        assert!(result.is_err(), "a source without colours must be rejected");
    }

    #[test]
    fn a_minimal_source_generates_every_section() {
        let source: Value = serde_json::json!({
            "version": 7,
            "color": { "tokens": { "surface.canvas": { "light": "#FFFFFF", "dark": "#000000" } } },
            "spacing": { "tokens": { "sm": 8 } },
            "radius": { "tokens": { "md": 12 } },
            "opacity": { "tokens": { "disabled": 0.38 } },
            "blur": { "tokens": { "regular": 20 } },
            "typography": { "tokens": { "body": { "size": 17, "weight": "regular", "textStyle": "body", "tracking": 0, "monospacedDigits": false } } },
            "motion": { "tokens": { "micro": { "duration": 120, "curve": "easeOut" } } },
            "elevation": { "tokens": { "raised": { "y": 1, "blur": 3, "opacity": 0.12 } } }
        });

        let swift = generate_swift(&source);
        assert!(swift.is_ok());
        let Ok(swift) = swift else { return };

        assert!(swift.contains("public static let version: Int = 7"));
        assert!(swift.contains("surfaceCanvas"));
        assert!(swift.contains("public enum Spacing"));
        assert!(swift.contains("public enum Radius"));
        assert!(swift.contains("public enum Opacity"));
        assert!(swift.contains("public enum Blur"));
        assert!(swift.contains("public enum Typography"));
        assert!(swift.contains("public enum Motion"));
        assert!(swift.contains("public enum Elevation"));
        assert!(swift.contains("DO NOT EDIT"));
    }
}

use core::fmt;

/// One of the twelve pitch classes, as semitones above C.
///
/// Enharmonic spellings (C sharp and D flat) collapse to one value. That is a
/// deliberate simplification: the distinction matters to a notating musician and
/// not at all to a DJ deciding whether two tracks sit together. Carrying it
/// would add a dimension to every comparison for no benefit at the point of use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum PitchClass {
    /// C.
    C = 0,
    /// C sharp, enharmonically D flat.
    CSharp = 1,
    /// D.
    D = 2,
    /// D sharp, enharmonically E flat.
    DSharp = 3,
    /// E.
    E = 4,
    /// F.
    F = 5,
    /// F sharp, enharmonically G flat.
    FSharp = 6,
    /// G.
    G = 7,
    /// G sharp, enharmonically A flat.
    GSharp = 8,
    /// A.
    A = 9,
    /// A sharp, enharmonically B flat.
    ASharp = 10,
    /// B.
    B = 11,
}

impl PitchClass {
    /// All twelve pitch classes in ascending order from C.
    pub const ALL: [Self; 12] = [
        Self::C,
        Self::CSharp,
        Self::D,
        Self::DSharp,
        Self::E,
        Self::F,
        Self::FSharp,
        Self::G,
        Self::GSharp,
        Self::A,
        Self::ASharp,
        Self::B,
    ];

    /// Returns the pitch class for a number of semitones above C.
    ///
    /// Values outside 0 to 11 wrap, so octave-equivalent input is handled
    /// without the caller needing to reduce it first.
    #[must_use]
    pub const fn from_semitones(semitones: u8) -> Self {
        match semitones % 12 {
            0 => Self::C,
            1 => Self::CSharp,
            2 => Self::D,
            3 => Self::DSharp,
            4 => Self::E,
            5 => Self::F,
            6 => Self::FSharp,
            7 => Self::G,
            8 => Self::GSharp,
            9 => Self::A,
            10 => Self::ASharp,
            _ => Self::B,
        }
    }

    /// Returns the number of semitones above C.
    #[must_use]
    pub const fn semitones(self) -> u8 {
        self as u8
    }

    /// The name using sharps, as used by most analysis tools.
    #[must_use]
    pub const fn sharp_name(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::CSharp => "C#",
            Self::D => "D",
            Self::DSharp => "D#",
            Self::E => "E",
            Self::F => "F",
            Self::FSharp => "F#",
            Self::G => "G",
            Self::GSharp => "G#",
            Self::A => "A",
            Self::ASharp => "A#",
            Self::B => "B",
        }
    }

    /// The name using flats, as used on most record sleeves.
    #[must_use]
    pub const fn flat_name(self) -> &'static str {
        match self {
            Self::C => "C",
            Self::CSharp => "Db",
            Self::D => "D",
            Self::DSharp => "Eb",
            Self::E => "E",
            Self::F => "F",
            Self::FSharp => "Gb",
            Self::G => "G",
            Self::GSharp => "Ab",
            Self::A => "A",
            Self::ASharp => "Bb",
            Self::B => "B",
        }
    }
}

impl fmt::Display for PitchClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.sharp_name())
    }
}

/// Major or minor.
///
/// Modal material — Dorian, Phrygian and the rest — is mapped to whichever of
/// the two it sits closest to for mixing purposes, with the detail retained in
/// the analysis record rather than here. Master Prompt #3A asks for modal
/// characteristics to be detected; this type is about the decision that follows
/// detection, and that decision turns on major or minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mode {
    /// Major.
    Major,
    /// Minor.
    Minor,
}

impl Mode {
    /// The other mode.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Major => Self::Minor,
            Self::Minor => Self::Major,
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Major => f.write_str("major"),
            Self::Minor => f.write_str("minor"),
        }
    }
}

/// A musical key: a tonic and a mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key {
    /// The tonic.
    pub tonic: PitchClass,
    /// Major or minor.
    pub mode: Mode,
}

impl Key {
    /// Creates a key.
    #[must_use]
    pub const fn new(tonic: PitchClass, mode: Mode) -> Self {
        Self { tonic, mode }
    }

    /// Creates a major key.
    #[must_use]
    pub const fn major(tonic: PitchClass) -> Self {
        Self::new(tonic, Mode::Major)
    }

    /// Creates a minor key.
    #[must_use]
    pub const fn minor(tonic: PitchClass) -> Self {
        Self::new(tonic, Mode::Minor)
    }

    /// Returns the relative key: the major or minor sharing the same notes.
    ///
    /// A minor and C major, for instance. The pair is interchangeable in a mix
    /// because it contains exactly the same pitches.
    #[must_use]
    pub const fn relative(self) -> Self {
        match self.mode {
            // The relative minor is three semitones below the major tonic.
            Mode::Major => Self::minor(PitchClass::from_semitones(
                (self.tonic.semitones() + 9) % 12,
            )),
            // The relative major is three semitones above the minor tonic.
            Mode::Minor => Self::major(PitchClass::from_semitones(
                (self.tonic.semitones() + 3) % 12,
            )),
        }
    }

    /// Returns the parallel key: the same tonic in the other mode.
    #[must_use]
    pub const fn parallel(self) -> Self {
        Self::new(self.tonic, self.mode.opposite())
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.tonic, self.mode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semitone_conversion_round_trips() {
        for pitch in PitchClass::ALL {
            assert_eq!(PitchClass::from_semitones(pitch.semitones()), pitch);
        }
    }

    #[test]
    fn semitones_wrap_across_octaves() {
        assert_eq!(PitchClass::from_semitones(12), PitchClass::C);
        assert_eq!(PitchClass::from_semitones(25), PitchClass::CSharp);
        assert_eq!(PitchClass::from_semitones(255), PitchClass::DSharp);
    }

    #[test]
    fn enharmonic_names_agree_on_the_naturals() {
        for pitch in [
            PitchClass::C,
            PitchClass::D,
            PitchClass::E,
            PitchClass::F,
            PitchClass::G,
            PitchClass::A,
            PitchClass::B,
        ] {
            assert_eq!(pitch.sharp_name(), pitch.flat_name());
        }
        assert_eq!(PitchClass::ASharp.flat_name(), "Bb");
        assert_eq!(PitchClass::ASharp.sharp_name(), "A#");
    }

    #[test]
    fn relative_keys_are_the_familiar_pairs() {
        assert_eq!(
            Key::major(PitchClass::C).relative(),
            Key::minor(PitchClass::A)
        );
        assert_eq!(
            Key::minor(PitchClass::A).relative(),
            Key::major(PitchClass::C)
        );
        assert_eq!(
            Key::major(PitchClass::G).relative(),
            Key::minor(PitchClass::E)
        );
        assert_eq!(
            Key::minor(PitchClass::F).relative(),
            Key::major(PitchClass::GSharp)
        );
    }

    #[test]
    fn taking_the_relative_twice_returns_the_original() {
        for tonic in PitchClass::ALL {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key::new(tonic, mode);
                assert_eq!(key.relative().relative(), key);
            }
        }
    }

    #[test]
    fn parallel_keys_share_a_tonic() {
        let a_minor = Key::minor(PitchClass::A);
        assert_eq!(a_minor.parallel(), Key::major(PitchClass::A));
        assert_eq!(a_minor.parallel().parallel(), a_minor);
    }

    #[test]
    fn keys_display_readably() {
        assert_eq!(Key::minor(PitchClass::A).to_string(), "A minor");
        assert_eq!(Key::major(PitchClass::FSharp).to_string(), "F# major");
    }
}

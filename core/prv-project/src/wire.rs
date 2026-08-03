//! The wire format: the bytes one device sends another.
//!
//! # Why the codec lives beside the operations rather than in `prv-sync`
//!
//! `prv-sync` owns synchronisation — the state machine, the outbox, the restore
//! points. The obvious home for a protocol is beside them, and it is the wrong
//! one, for the same reason [`Interpolation::apply`](crate::Interpolation::apply)
//! lives with the enum it interprets: the encoder must handle *every* variant of
//! [`OperationPayload`], and only the crate that defines that enum can have the
//! compiler say so.
//!
//! Written here, adding a variant fails to compile until it has a number and an
//! encoding. Written one crate away, [`OperationPayload`] is `non_exhaustive`,
//! the match needs a wildcard arm, and a new kind of edit would ship as an
//! operation that silently refuses to travel — discovered by a user whose work
//! did not arrive. ADR-0003's rule (a variant is never redefined and never
//! removed) is a rule *about this file*, so this is where it is kept.
//!
//! Encoding is not I/O. Nothing here opens, reads or writes anything; it turns
//! values into bytes and back, which leaves ADR-0001 intact.
//!
//! # What the bytes are
//!
//! Everything is little-endian and fixed-width. A message is a header followed
//! by a run of length-prefixed entries:
//!
//! ```text
//! message  magic "PRVL" | major u16 | minor u16 | header u32 length | header
//!          | entries, back to back
//! header   entries u32
//! entry    length u32 | body
//! body     device u64 | sequence u64 | timestamp i64
//!          | context u32 count, then (device u64, sequence u64) pairs
//!          | kind u16 | payload u32 length | payload bytes
//! ```
//!
//! The version vector is written in device order, because it is stored in a
//! `BTreeMap` and read back through [`VersionVector::entries`]. That is what
//! makes the encoding *canonical*: the same operations always produce the same
//! bytes, on every platform, so a message can be compared, cached or signed.
//!
//! # Three places it can grow
//!
//! Each of the three lengths above is a place a later build can add something an
//! earlier one walks past:
//!
//! **The header length** covers fields about the message as a whole. Everything
//! after the entry count is ignored by this build and copied verbatim when the
//! message is passed on.
//!
//! **The entry length** covers fields about one operation.
//!
//! **The payload length** covers the operation itself, including payload kinds
//! this build has never heard of.
//!
//! The minor version is the announcement, not the mechanism. It rises when a
//! message gains something a reader may ignore, and it travels with the message
//! rather than being restamped by whoever passes it on — a relay that wrote its
//! own minor version would be claiming the contents were older than they are.
//! A differing *major* version is refused outright; a differing minor is not,
//! which is the whole point of having two numbers.
//!
//! Without the header length the minor version would be a promise the format
//! could not keep: a reader that met an added header field would have no way to
//! find where the entries began, and "additive" would mean "unreadable".
//!
//! # Understand all of it, or none of it
//!
//! A reader that meets something it does not know does not guess and does not
//! discard. It keeps the entry's bytes exactly as they arrived, reports it, and
//! passes it on unchanged — so a device running last year's build is a *relay*
//! rather than a hole in the fleet. Three devices, one of them stale, still
//! converge, because the stale one carries what it cannot read.
//!
//! What it will not do is apply half of it. If an entry carries a payload this
//! build knows but with bytes left over — a field a newer build appended — the
//! whole entry is treated as not understood. Applying the part we recognise
//! would mean storing an edit stripped of the meaning its author gave it, and
//! then relaying our stripped version as though it were theirs.
//!
//! The practical consequence for anyone extending this: **prefer a new variant
//! over a new field on an existing one.** Both are safe. A new variant is
//! ignored by old builds; a new field on an existing variant makes old builds
//! stop applying that variant entirely.
//!
//! # Malformed, or merely newer?
//!
//! The distinction decides whether a message is rejected or relayed, so it is
//! drawn on one question: *could any build, ever, have meant this?*
//!
//! - Bytes that end mid-field, text that is not UTF-8, a boolean that is neither
//!   zero nor one, a value that is not a finite number — no build meant these.
//!   They are [`WireError`], and they fail the message.
//! - A discriminant we do not recognise, or a value outside a bound this build
//!   happens to enforce — a later build may have widened either. These make the
//!   entry unrecognised, and it travels on.
//!
//! Every tag in this format starts at one, so a byte of zero is never a valid
//! discriminant. A buffer that was zeroed, or truncated and padded, therefore
//! fails loudly instead of decoding into a plausible-looking edit.
//!
//! # Allocation is bounded by what actually arrived
//!
//! Every count in the format is checked against the bytes remaining before it is
//! used to reserve anything. A message claiming four billion entries in twelve
//! bytes is refused without allocating, because four billion entries cannot fit
//! in twelve bytes. That check — not a fixed limit — is the guard, and it is
//! exact: a sender can only make a receiver allocate in proportion to what it
//! was willing to send.
//!
//! # Integrity is the transport's job
//!
//! There is no checksum here. Master Prompt #26 requires the transport to be
//! encrypted and authenticated, which detects tampering and corruption far
//! better than a checksum can — and a checksum beside real authentication mostly
//! provides somewhere for a reader's confidence to come from when it should not.

use core::fmt;

use prv_time::{Frames, Tempo};

use crate::operation::{
    DeviceId, MarkerId, MarkerKind, Operation, OperationId, OperationPayload, PlacementId,
    TrackRef, VersionVector,
};
use crate::parameter::{
    Interpolation, ParameterAddress, ParameterKey, ParameterOwner, PluginParameterId,
};

/// The first four bytes of every message.
///
/// Present so that a file or a socket carrying something else is rejected on the
/// first read rather than interpreted as an enormous version vector.
pub const MAGIC: [u8; 4] = *b"PRVL";

/// The first four bytes of a version vector sent on its own.
///
/// Distinct from [`MAGIC`] so that the two messages of the protocol cannot be
/// mistaken for one another — a vector read as a log would be a log with no
/// operations, which is a silent and very confusing way to synchronise nothing.
pub const VECTOR_MAGIC: [u8; 4] = *b"PRVV";

/// The format version whose meaning must match for a message to be read.
pub const FORMAT_MAJOR: u16 = 1;

/// The format version that rises when something ignorable is added.
pub const FORMAT_MINOR: u16 = 0;

/// The longest a name or a label may be, in bytes.
///
/// Four kilobytes: past anything a person types, short enough to bound what one
/// field can cost. The presentation layer should stop a user long before this —
/// a limit reached here is a limit reached after the work was already done, and
/// [`encode`] can only refuse.
pub const MAX_TEXT_BYTES: usize = 4096;

/// Bytes a message costs before its first entry: magic, major, minor, the
/// header's own length, and the entry count inside it.
const HEADER_BYTES: usize = 4 + 2 + 2 + 4 + 4;

/// The fewest bytes an entry can occupy, including its own length prefix.
///
/// Length, device, sequence, timestamp, an empty context, a kind and an empty
/// payload. Used to reject an implausible entry count before reserving for it.
const MIN_ENTRY_BYTES: usize = 4 + 8 + 8 + 8 + 4 + 2 + 4;

/// Bytes in one version vector entry.
const VECTOR_ENTRY_BYTES: usize = 8 + 8;

// Payload discriminants. Assigned once and never reused: a number that meant
// something in a shipped build means that forever, because a project written by
// an earlier build must keep its meaning.
const KIND_SET_PROJECT_NAME: u16 = 1;
const KIND_PLACE_TRACK: u16 = 2;
const KIND_MOVE_PLACEMENT: u16 = 3;
const KIND_TRIM_PLACEMENT: u16 = 4;
const KIND_REMOVE_PLACEMENT: u16 = 5;
const KIND_ADD_MARKER: u16 = 6;
const KIND_REMOVE_MARKER: u16 = 7;
const KIND_SET_AUTOMATION_POINT: u16 = 8;
const KIND_REMOVE_AUTOMATION_POINT: u16 = 9;
const KIND_SET_PLACEMENT_SOURCE: u16 = 10;
const KIND_SET_TEMPO: u16 = 11;
const KIND_REMOVE_TEMPO: u16 = 12;
const KIND_SET_AUTOMATION_ENABLED: u16 = 13;

// Marker kinds.
const MARKER_CUE: u8 = 1;
const MARKER_BUILD_UP: u8 = 2;
const MARKER_DROP: u8 = 3;
const MARKER_BREAKDOWN: u8 = 4;
const MARKER_NOTE: u8 = 5;

// Interpolation shapes.
const INTERPOLATION_HOLD: u8 = 1;
const INTERPOLATION_LINEAR: u8 = 2;
const INTERPOLATION_SMOOTH: u8 = 3;
const INTERPOLATION_ACCELERATING: u8 = 4;
const INTERPOLATION_DECELERATING: u8 = 5;

// Parameter owners.
const OWNER_MASTER: u8 = 1;
const OWNER_LANE: u8 = 2;
const OWNER_PLACEMENT: u8 = 3;

// Parameter keys.
const KEY_GAIN: u8 = 1;
const KEY_EQ_LOW: u8 = 2;
const KEY_EQ_MID: u8 = 3;
const KEY_EQ_HIGH: u8 = 4;
const KEY_FILTER: u8 = 5;
const KEY_MIX: u8 = 6;
const KEY_CROSSFADER: u8 = 7;
const KEY_PLUGIN: u8 = 8;

/// Something that makes a run of bytes not a message.
///
/// Every variant here means *no build ever meant this*. Anything a later build
/// might legitimately have meant becomes an unrecognised entry instead, which is
/// carried rather than refused; see the module documentation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum WireError {
    /// The bytes do not begin with [`MAGIC`].
    NotAMessage,
    /// A major version this build cannot read.
    UnsupportedVersion {
        /// The major version the message declares.
        major: u16,
        /// The major version this build speaks.
        expected: u16,
    },
    /// The bytes end in the middle of a field.
    Truncated,
    /// Bytes after the last entry, which no reader can account for.
    TrailingBytes {
        /// How many were left over.
        count: usize,
    },
    /// A count larger than the bytes present could possibly hold.
    ///
    /// The guard that stops a short message from asking for an enormous
    /// allocation.
    ImplausibleCount {
        /// The count the message declared.
        claimed: u64,
        /// The bytes actually available to hold it.
        available: usize,
    },
    /// Text that is not valid UTF-8.
    InvalidText,
    /// A boolean encoded as something other than zero or one.
    InvalidBoolean {
        /// The byte found.
        value: u8,
    },
    /// A number that is not finite, where only a finite one has meaning.
    ValueNotFinite,
    /// An operation numbered zero, which no device ever issues.
    InvalidSequence {
        /// The device that supposedly issued it.
        device: DeviceId,
    },
    /// Text longer than [`MAX_TEXT_BYTES`], refused while encoding.
    TextTooLong {
        /// The length offered.
        length: usize,
        /// The longest permitted.
        maximum: usize,
    },
    /// A message that will not fit the format's own counters.
    TooLarge,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAMessage => f.write_str("not a project message"),
            Self::UnsupportedVersion { major, expected } => write!(
                f,
                "message format {major} cannot be read by a build that speaks {expected}"
            ),
            Self::Truncated => f.write_str("the message ends in the middle of a field"),
            Self::TrailingBytes { count } => {
                write!(f, "{count} bytes after the last operation")
            }
            Self::ImplausibleCount { claimed, available } => write!(
                f,
                "the message claims {claimed} items but carries only {available} bytes to hold them"
            ),
            Self::InvalidText => f.write_str("text in the message is not valid UTF-8"),
            Self::InvalidBoolean { value } => {
                write!(f, "{value} is neither true nor false")
            }
            Self::ValueNotFinite => f.write_str("a value in the message is not a finite number"),
            Self::InvalidSequence { device } => {
                write!(f, "{device} issued an operation numbered zero")
            }
            Self::TextTooLong { length, maximum } => write!(
                f,
                "text is {length} bytes, the most that can be sent is {maximum}"
            ),
            Self::TooLarge => f.write_str("the message is too large for the format"),
        }
    }
}

impl core::error::Error for WireError {}

/// An entry this build could not fully understand.
///
/// Kept byte for byte so it can be passed on exactly as it arrived. Its
/// identity, context and timestamp are read out because those fields are part of
/// the envelope every build shares — which is what lets a stale device order,
/// deduplicate and report an operation whose meaning it does not know.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unrecognised {
    id: OperationId,
    context: VersionVector,
    timestamp_micros: i64,
    kind: u16,
    /// The entry body exactly as received, including the fields above.
    ///
    /// They are stored twice: once parsed, for reporting, and once raw, so that
    /// passing the entry on cannot alter a single byte of it. Re-encoding from
    /// the parsed fields would be a re-encoding by *this* build, which is
    /// precisely what must not happen to something this build does not
    /// understand.
    bytes: Vec<u8>,
}

impl Unrecognised {
    /// Who made it, and when in their own numbering.
    #[must_use]
    pub const fn id(&self) -> OperationId {
        self.id
    }

    /// What its author had already seen.
    #[must_use]
    pub const fn context(&self) -> &VersionVector {
        &self.context
    }

    /// The author's wall clock, in microseconds since the epoch.
    #[must_use]
    pub const fn timestamp_micros(&self) -> i64 {
        self.timestamp_micros
    }

    /// The payload discriminant this build does not know.
    ///
    /// Zero when the payload *was* recognised but the entry carried fields
    /// beyond it — the entry is still not understood, but the kind is not what
    /// was unfamiliar.
    #[must_use]
    pub const fn kind(&self) -> u16 {
        self.kind
    }

    /// How many bytes it occupies.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.bytes.len()
    }
}

/// One item of a message.
#[derive(Debug, Clone, PartialEq)]
pub enum Entry {
    /// An operation this build can apply.
    Understood(Operation),
    /// An operation this build can only carry.
    Unrecognised(Unrecognised),
}

/// A decoded message.
///
/// Entries are held in the order they arrived, including the ones that were not
/// understood, so that [`Message::encode`] reproduces the message it came from
/// byte for byte.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Message {
    entries: Vec<Entry>,
    minor: u16,
    /// Header fields a newer build added, kept so passing the message on does
    /// not quietly strip them.
    header_extra: Vec<u8>,
}

impl Message {
    /// Every entry, in the order received.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The operations that can be applied.
    pub fn understood(&self) -> impl Iterator<Item = &Operation> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Understood(operation) => Some(operation),
            Entry::Unrecognised(_) => None,
        })
    }

    /// The entries that can only be carried.
    pub fn unrecognised(&self) -> impl Iterator<Item = &Unrecognised> {
        self.entries.iter().filter_map(|entry| match entry {
            Entry::Unrecognised(unknown) => Some(unknown),
            Entry::Understood(_) => None,
        })
    }

    /// How many entries could only be carried.
    ///
    /// Worth showing a user. "Four edits were made in a newer version of the
    /// app; they are being kept and passed on, but cannot be shown here" is a
    /// true and actionable sentence, and it is the only honest thing to say.
    #[must_use]
    pub fn unrecognised_count(&self) -> usize {
        self.unrecognised().count()
    }

    /// The operations ready to hand to [`OperationLog::merge`](crate::OperationLog::merge).
    #[must_use]
    pub fn to_operations(&self) -> Vec<Operation> {
        self.understood().cloned().collect()
    }

    /// How many entries there are.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there is nothing to apply and nothing to carry.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The minor version the message declared.
    #[must_use]
    pub const fn minor(&self) -> u16 {
        self.minor
    }

    /// Whether the header carried fields this build does not read.
    ///
    /// They are carried rather than refused — that is what a minor version
    /// means — but a reader that wants to say "this came from a newer build"
    /// needs to be able to tell.
    #[must_use]
    pub fn carries_unread_header_fields(&self) -> bool {
        !self.header_extra.is_empty()
    }

    /// Writes the message out again.
    ///
    /// Entries that were understood are re-encoded; entries that were not are
    /// copied. A message decoded and re-encoded by a build that understands all
    /// of it, or none of it, produces exactly the bytes it was given.
    ///
    /// # Errors
    ///
    /// Returns [`WireError`] if a name or label exceeds [`MAX_TEXT_BYTES`], or
    /// if the message will not fit the format's counters.
    pub fn encode(&self) -> Result<Vec<u8>, WireError> {
        let mut out = begin(self.entries.len(), self.minor, &self.header_extra)?;
        let mut entry = Vec::new();
        let mut body = Vec::new();
        for item in &self.entries {
            match item {
                Entry::Understood(operation) => {
                    entry.clear();
                    write_operation(&mut entry, &mut body, operation)?;
                    push_frame(&mut out, &entry)?;
                }
                Entry::Unrecognised(unknown) => push_frame(&mut out, &unknown.bytes)?,
            }
        }
        Ok(out)
    }
}

/// Turns a version vector into the bytes that carry it.
///
/// The other half of the protocol, and the half that goes first. A device says
/// what it has seen; the other side answers with
/// [`OperationLog::operations_since`](crate::OperationLog::operations_since).
/// That exchange is what makes synchronisation incremental rather than a full
/// copy every time two machines meet, and it is why a vector needs a message of
/// its own rather than only appearing inside operations.
///
/// # Errors
///
/// Returns [`WireError::TooLarge`] for a vector naming more devices than the
/// format can count.
pub fn encode_vector(vector: &VersionVector) -> Result<Vec<u8>, WireError> {
    let mut body = Vec::new();
    put_vector(&mut body, vector)?;

    let mut out = Vec::new();
    out.extend_from_slice(&VECTOR_MAGIC);
    put_u16(&mut out, FORMAT_MAJOR);
    put_u16(&mut out, FORMAT_MINOR);
    push_frame(&mut out, &body)?;
    Ok(out)
}

/// Reads a version vector another device sent.
///
/// # Errors
///
/// Returns [`WireError`] when the bytes are not a vector this build could have
/// been sent.
pub fn decode_vector(bytes: &[u8]) -> Result<VersionVector, WireError> {
    let mut reader = Reader::new(bytes);
    if reader.array::<4>()? != VECTOR_MAGIC {
        return Err(WireError::NotAMessage);
    }
    let major = reader.u16()?;
    if major != FORMAT_MAJOR {
        return Err(WireError::UnsupportedVersion {
            major,
            expected: FORMAT_MAJOR,
        });
    }
    let _minor = reader.u16()?;

    // Framed for the same reason the message header is: a newer build may say
    // more about what it has seen, and this one steps over it. Nothing is kept,
    // because a vector is regenerated at every exchange rather than passed on —
    // there is no third party whose meaning would be lost.
    let length = reader.counted(1)?;
    let mut body = Reader::new(reader.take(length)?);
    let vector = body.vector()?;

    if reader.remaining() != 0 {
        return Err(WireError::TrailingBytes {
            count: reader.remaining(),
        });
    }
    Ok(vector)
}

/// Turns operations into the bytes that carry them.
///
/// # Errors
///
/// Returns [`WireError::TextTooLong`] for a name or label beyond
/// [`MAX_TEXT_BYTES`], or [`WireError::TooLarge`] for a message that will not
/// fit the format's counters.
pub fn encode(operations: &[Operation]) -> Result<Vec<u8>, WireError> {
    let mut out = begin(operations.len(), FORMAT_MINOR, &[])?;
    let mut entry = Vec::new();
    let mut body = Vec::new();
    for operation in operations {
        entry.clear();
        write_operation(&mut entry, &mut body, operation)?;
        push_frame(&mut out, &entry)?;
    }
    Ok(out)
}

/// Reads bytes that arrived from somewhere else.
///
/// # Errors
///
/// Returns [`WireError`] when the bytes are not a message this build could have
/// been sent. An operation this build does not understand is *not* an error; it
/// becomes an [`Unrecognised`] entry.
pub fn decode(bytes: &[u8]) -> Result<Message, WireError> {
    let mut reader = Reader::new(bytes);
    if reader.array::<4>()? != MAGIC {
        return Err(WireError::NotAMessage);
    }
    let major = reader.u16()?;
    if major != FORMAT_MAJOR {
        return Err(WireError::UnsupportedVersion {
            major,
            expected: FORMAT_MAJOR,
        });
    }
    let minor = reader.u16()?;

    // The header is a frame of its own, so a field added by a newer build is
    // something to step over rather than something that hides where the entries
    // begin.
    let header_length = reader.counted(1)?;
    let mut header = Reader::new(reader.take(header_length)?);
    let claimed = header.u32()?;
    let header_extra = header.rest().to_vec();

    // Checked against what follows the header, not against what is left inside
    // it: the entries live outside the frame the count was read from.
    let count = plausible(claimed, MIN_ENTRY_BYTES, reader.remaining())?;

    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        entries.push(read_entry(&mut reader)?);
    }
    if reader.remaining() != 0 {
        return Err(WireError::TrailingBytes {
            count: reader.remaining(),
        });
    }
    Ok(Message {
        entries,
        minor,
        header_extra,
    })
}

/// Starts a message, reserving room for its entries.
fn begin(count: usize, minor: u16, extra: &[u8]) -> Result<Vec<u8>, WireError> {
    let count = u32::try_from(count).map_err(|_| WireError::TooLarge)?;
    let mut header = Vec::with_capacity(4usize.saturating_add(extra.len()));
    put_u32(&mut header, count);
    header.extend_from_slice(extra);

    let mut out = Vec::with_capacity(HEADER_BYTES.saturating_add(extra.len()));
    out.extend_from_slice(&MAGIC);
    put_u16(&mut out, FORMAT_MAJOR);
    put_u16(&mut out, minor);
    push_frame(&mut out, &header)?;
    Ok(out)
}

/// Refuses a count the bytes present could not possibly hold.
///
/// The allocation guard, and the only one: `unit` is the fewest bytes one item
/// can occupy, so a sender can make a receiver reserve room only in proportion
/// to what it was willing to send. A fixed ceiling would be both looser than
/// this — it would still permit reserving the ceiling from a short message — and
/// tighter, since it would refuse a large message that is entirely legitimate.
fn plausible(claimed: u32, unit: usize, available: usize) -> Result<usize, WireError> {
    let refuse = || WireError::ImplausibleCount {
        claimed: u64::from(claimed),
        available,
    };
    let count = usize::try_from(claimed).map_err(|_| WireError::TooLarge)?;
    if count.checked_mul(unit).ok_or_else(refuse)? > available {
        return Err(refuse());
    }
    Ok(count)
}

/// Appends a length-prefixed frame.
fn push_frame(out: &mut Vec<u8>, body: &[u8]) -> Result<(), WireError> {
    let length = u32::try_from(body.len()).map_err(|_| WireError::TooLarge)?;
    put_u32(out, length);
    out.extend_from_slice(body);
    Ok(())
}

// ---------------------------------------------------------------------------
// Writing
// ---------------------------------------------------------------------------

fn put_u8(out: &mut Vec<u8>, value: u8) {
    out.push(value);
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_frames(out: &mut Vec<u8>, value: Frames) {
    put_i64(out, value.get());
}

fn put_bool(out: &mut Vec<u8>, value: bool) {
    put_u8(out, u8::from(value));
}

fn put_f32(out: &mut Vec<u8>, value: f32) -> Result<(), WireError> {
    if !value.is_finite() {
        return Err(WireError::ValueNotFinite);
    }
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn put_text(out: &mut Vec<u8>, value: &str) -> Result<(), WireError> {
    if value.len() > MAX_TEXT_BYTES {
        return Err(WireError::TextTooLong {
            length: value.len(),
            maximum: MAX_TEXT_BYTES,
        });
    }
    let length = u32::try_from(value.len()).map_err(|_| WireError::TooLarge)?;
    put_u32(out, length);
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn put_vector(out: &mut Vec<u8>, vector: &VersionVector) -> Result<(), WireError> {
    let count = u32::try_from(vector.device_count()).map_err(|_| WireError::TooLarge)?;
    put_u32(out, count);
    for (device, sequence) in vector.entries() {
        put_u64(out, device.get());
        put_u64(out, sequence);
    }
    Ok(())
}

/// Writes one entry body: envelope, then the payload in its own frame.
fn write_operation(
    out: &mut Vec<u8>,
    body: &mut Vec<u8>,
    operation: &Operation,
) -> Result<(), WireError> {
    put_u64(out, operation.id.device.get());
    put_u64(out, operation.id.sequence);
    put_i64(out, operation.timestamp_micros);
    put_vector(out, &operation.context)?;

    body.clear();
    let kind = write_payload(body, &operation.payload)?;
    put_u16(out, kind);
    push_frame(out, body)
}

/// Writes a payload and returns its discriminant.
///
/// The match is exhaustive over [`OperationPayload`] and has no wildcard arm.
/// That is the point of the module living in this crate: a new variant does not
/// compile until it is given a number here.
fn write_payload(out: &mut Vec<u8>, payload: &OperationPayload) -> Result<u16, WireError> {
    match payload {
        OperationPayload::SetProjectName { name } => {
            put_text(out, name)?;
            Ok(KIND_SET_PROJECT_NAME)
        }
        OperationPayload::PlaceTrack {
            placement,
            track,
            position,
            length,
            lane,
        } => {
            put_u64(out, placement.get());
            put_u64(out, track.get());
            put_frames(out, *position);
            put_frames(out, *length);
            put_u32(out, *lane);
            Ok(KIND_PLACE_TRACK)
        }
        OperationPayload::MovePlacement {
            placement,
            position,
            lane,
        } => {
            put_u64(out, placement.get());
            put_frames(out, *position);
            put_u32(out, *lane);
            Ok(KIND_MOVE_PLACEMENT)
        }
        OperationPayload::TrimPlacement { placement, length } => {
            put_u64(out, placement.get());
            put_frames(out, *length);
            Ok(KIND_TRIM_PLACEMENT)
        }
        OperationPayload::RemovePlacement { placement } => {
            put_u64(out, placement.get());
            Ok(KIND_REMOVE_PLACEMENT)
        }
        OperationPayload::AddMarker {
            marker,
            position,
            kind,
            label,
        } => {
            put_u64(out, marker.get());
            put_frames(out, *position);
            put_u8(out, marker_kind_code(*kind));
            put_text(out, label)?;
            Ok(KIND_ADD_MARKER)
        }
        OperationPayload::RemoveMarker { marker } => {
            put_u64(out, marker.get());
            Ok(KIND_REMOVE_MARKER)
        }
        OperationPayload::SetAutomationPoint {
            address,
            position,
            value,
            interpolation,
        } => {
            put_address(out, address)?;
            put_frames(out, *position);
            put_f32(out, *value)?;
            put_u8(out, interpolation_code(*interpolation));
            Ok(KIND_SET_AUTOMATION_POINT)
        }
        OperationPayload::RemoveAutomationPoint { address, position } => {
            put_address(out, address)?;
            put_frames(out, *position);
            Ok(KIND_REMOVE_AUTOMATION_POINT)
        }
        OperationPayload::SetPlacementSource {
            placement,
            source_offset,
        } => {
            put_u64(out, placement.get());
            put_frames(out, *source_offset);
            Ok(KIND_SET_PLACEMENT_SOURCE)
        }
        OperationPayload::SetTempo { position, tempo } => {
            put_frames(out, *position);
            put_u64(out, tempo.micros_per_beat());
            Ok(KIND_SET_TEMPO)
        }
        OperationPayload::RemoveTempo { position } => {
            put_frames(out, *position);
            Ok(KIND_REMOVE_TEMPO)
        }
        OperationPayload::SetAutomationEnabled { address, enabled } => {
            put_address(out, address)?;
            put_bool(out, *enabled);
            Ok(KIND_SET_AUTOMATION_ENABLED)
        }
    }
}

/// Writes a parameter address as a base owner, a chain of effect slots, and a
/// key.
///
/// Flattened deliberately. The owner is a tree, and a decoder that rebuilt it by
/// recursion would recurse as deeply as an attacker asked. Written flat, the
/// chain is bounded by a byte and checked against
/// [`ParameterOwner::MAX_DEPTH`](crate::ParameterOwner::MAX_DEPTH) before
/// anything is built.
fn put_address(out: &mut Vec<u8>, address: &ParameterAddress) -> Result<(), WireError> {
    let mut slots = Vec::new();
    let mut owner = address.owner();
    while let ParameterOwner::Effect { host, slot } = owner {
        slots.push(*slot);
        owner = host;
    }
    match owner {
        ParameterOwner::Master => put_u8(out, OWNER_MASTER),
        ParameterOwner::Lane(index) => {
            put_u8(out, OWNER_LANE);
            put_u32(out, *index);
        }
        ParameterOwner::Placement(id) => {
            put_u8(out, OWNER_PLACEMENT);
            put_u64(out, id.get());
        }
        // Unreachable: the loop above walks past every effect before this match
        // runs. Written as an arm rather than a wildcard so that a new owner
        // shape is a compile error here.
        ParameterOwner::Effect { .. } => return Err(WireError::TooLarge),
    }

    let depth = u8::try_from(slots.len()).map_err(|_| WireError::TooLarge)?;
    put_u8(out, depth);
    // The walk collected the chain from the outside in; the reader rebuilds it
    // from the base outward.
    for slot in slots.iter().rev() {
        put_u8(out, *slot);
    }

    match address.key() {
        ParameterKey::Gain => put_u8(out, KEY_GAIN),
        ParameterKey::EqLow => put_u8(out, KEY_EQ_LOW),
        ParameterKey::EqMid => put_u8(out, KEY_EQ_MID),
        ParameterKey::EqHigh => put_u8(out, KEY_EQ_HIGH),
        ParameterKey::Filter => put_u8(out, KEY_FILTER),
        ParameterKey::Mix => put_u8(out, KEY_MIX),
        ParameterKey::Crossfader => put_u8(out, KEY_CROSSFADER),
        ParameterKey::Plugin(id) => {
            put_u8(out, KEY_PLUGIN);
            put_text(out, id.as_str())?;
        }
    }
    Ok(())
}

const fn marker_kind_code(kind: MarkerKind) -> u8 {
    match kind {
        MarkerKind::Cue => MARKER_CUE,
        MarkerKind::BuildUp => MARKER_BUILD_UP,
        MarkerKind::Drop => MARKER_DROP,
        MarkerKind::Breakdown => MARKER_BREAKDOWN,
        MarkerKind::Note => MARKER_NOTE,
    }
}

const fn interpolation_code(interpolation: Interpolation) -> u8 {
    match interpolation {
        Interpolation::Hold => INTERPOLATION_HOLD,
        Interpolation::Linear => INTERPOLATION_LINEAR,
        Interpolation::Smooth => INTERPOLATION_SMOOTH,
        Interpolation::Accelerating => INTERPOLATION_ACCELERATING,
        Interpolation::Decelerating => INTERPOLATION_DECELERATING,
    }
}

// ---------------------------------------------------------------------------
// Reading
// ---------------------------------------------------------------------------

/// A cursor over bytes that arrived from somewhere else.
///
/// Every read is checked. There is no indexing in this module, so the worst a
/// hostile message can do is produce a [`WireError`].
#[derive(Debug)]
struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], WireError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or(WireError::Truncated)?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or(WireError::Truncated)?;
        self.position = end;
        Ok(slice)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], WireError> {
        let slice = self.take(N)?;
        <[u8; N]>::try_from(slice).map_err(|_| WireError::Truncated)
    }

    fn u8(&mut self) -> Result<u8, WireError> {
        Ok(u8::from_le_bytes(self.array::<1>()?))
    }

    fn u16(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_le_bytes(self.array::<2>()?))
    }

    fn u32(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_le_bytes(self.array::<4>()?))
    }

    fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(self.array::<8>()?))
    }

    fn i64(&mut self) -> Result<i64, WireError> {
        Ok(i64::from_le_bytes(self.array::<8>()?))
    }

    fn frames(&mut self) -> Result<Frames, WireError> {
        Ok(Frames::new(self.i64()?))
    }

    fn boolean(&mut self) -> Result<bool, WireError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(WireError::InvalidBoolean { value }),
        }
    }

    fn f32(&mut self) -> Result<f32, WireError> {
        let value = f32::from_le_bytes(self.array::<4>()?);
        if value.is_finite() {
            Ok(value)
        } else {
            Err(WireError::ValueNotFinite)
        }
    }

    /// Reads a count and refuses it if the bytes still ahead could not hold it.
    fn counted(&mut self, unit: usize) -> Result<usize, WireError> {
        let claimed = self.u32()?;
        plausible(claimed, unit, self.remaining())
    }

    /// Everything not yet read, consumed.
    fn rest(&mut self) -> &'a [u8] {
        let remaining = self.remaining();
        self.take(remaining).unwrap_or_default()
    }

    fn text(&mut self) -> Result<&'a str, WireError> {
        let length = self.counted(1)?;
        let bytes = self.take(length)?;
        core::str::from_utf8(bytes).map_err(|_| WireError::InvalidText)
    }

    fn vector(&mut self) -> Result<VersionVector, WireError> {
        let count = self.counted(VECTOR_ENTRY_BYTES)?;
        let mut vector = VersionVector::new();
        for _ in 0..count {
            let device = DeviceId::new(self.u64()?);
            let sequence = self.u64()?;
            vector.observe(OperationId::new(device, sequence));
        }
        Ok(vector)
    }
}

/// What reading a payload produced.
///
/// The third state is the one that matters: bytes that are well-formed but mean
/// something this build has no name for.
enum Understanding {
    /// A payload this build can apply.
    Known(OperationPayload),
    /// Well-formed bytes whose meaning belongs to a newer build.
    Newer,
}

/// Reads one entry, understanding it if it can and preserving it if it cannot.
fn read_entry(reader: &mut Reader<'_>) -> Result<Entry, WireError> {
    let length = reader.counted(1)?;
    let bytes = reader.take(length)?;
    let mut entry = Reader::new(bytes);

    let device = DeviceId::new(entry.u64()?);
    let sequence = entry.u64()?;
    if sequence == 0 {
        return Err(WireError::InvalidSequence { device });
    }
    let id = OperationId::new(device, sequence);
    let timestamp_micros = entry.i64()?;
    let context = entry.vector()?;

    let kind = entry.u16()?;
    let payload_length = entry.counted(1)?;
    let payload_bytes = entry.take(payload_length)?;

    let carry = |kind: u16| {
        Ok(Entry::Unrecognised(Unrecognised {
            id,
            context: context.clone(),
            timestamp_micros,
            kind,
            bytes: bytes.to_vec(),
        }))
    };

    // Fields appended to the entry by a newer build. The payload may well be one
    // this build knows, and applying it anyway would store an edit stripped of
    // whatever those fields meant.
    if entry.remaining() != 0 {
        return carry(0);
    }

    let mut payload = Reader::new(payload_bytes);
    match read_payload(kind, &mut payload)? {
        // Bytes left inside a payload this build recognises: the same situation
        // one level down, and the same answer.
        Understanding::Known(_) if payload.remaining() != 0 => carry(0),
        Understanding::Known(payload) => Ok(Entry::Understood(Operation {
            id,
            context,
            timestamp_micros,
            payload,
        })),
        Understanding::Newer => carry(kind),
    }
}

/// Reads a payload of a known kind, or reports that the kind is not known.
fn read_payload(kind: u16, reader: &mut Reader<'_>) -> Result<Understanding, WireError> {
    let payload = match kind {
        KIND_SET_PROJECT_NAME => OperationPayload::SetProjectName {
            name: reader.text()?.to_owned(),
        },
        KIND_PLACE_TRACK => OperationPayload::PlaceTrack {
            placement: PlacementId::new(reader.u64()?),
            track: TrackRef::new(reader.u64()?),
            position: reader.frames()?,
            length: reader.frames()?,
            lane: reader.u32()?,
        },
        KIND_MOVE_PLACEMENT => OperationPayload::MovePlacement {
            placement: PlacementId::new(reader.u64()?),
            position: reader.frames()?,
            lane: reader.u32()?,
        },
        KIND_TRIM_PLACEMENT => OperationPayload::TrimPlacement {
            placement: PlacementId::new(reader.u64()?),
            length: reader.frames()?,
        },
        KIND_REMOVE_PLACEMENT => OperationPayload::RemovePlacement {
            placement: PlacementId::new(reader.u64()?),
        },
        KIND_ADD_MARKER => {
            let marker = MarkerId::new(reader.u64()?);
            let position = reader.frames()?;
            let Some(kind) = marker_kind(reader.u8()?) else {
                return Ok(Understanding::Newer);
            };
            OperationPayload::AddMarker {
                marker,
                position,
                kind,
                label: reader.text()?.to_owned(),
            }
        }
        KIND_REMOVE_MARKER => OperationPayload::RemoveMarker {
            marker: MarkerId::new(reader.u64()?),
        },
        KIND_SET_AUTOMATION_POINT => {
            let Some(address) = read_address(reader)? else {
                return Ok(Understanding::Newer);
            };
            let position = reader.frames()?;
            let value = reader.f32()?;
            let Some(interpolation) = interpolation(reader.u8()?) else {
                return Ok(Understanding::Newer);
            };
            OperationPayload::SetAutomationPoint {
                address,
                position,
                value,
                interpolation,
            }
        }
        KIND_REMOVE_AUTOMATION_POINT => {
            let Some(address) = read_address(reader)? else {
                return Ok(Understanding::Newer);
            };
            OperationPayload::RemoveAutomationPoint {
                address,
                position: reader.frames()?,
            }
        }
        KIND_SET_PLACEMENT_SOURCE => OperationPayload::SetPlacementSource {
            placement: PlacementId::new(reader.u64()?),
            source_offset: reader.frames()?,
        },
        KIND_SET_TEMPO => {
            let position = reader.frames()?;
            let Ok(tempo) = Tempo::from_micros_per_beat(reader.u64()?) else {
                // A tempo outside this build's range. A later build may permit
                // a wider one, so the entry travels rather than failing.
                return Ok(Understanding::Newer);
            };
            OperationPayload::SetTempo { position, tempo }
        }
        KIND_REMOVE_TEMPO => OperationPayload::RemoveTempo {
            position: reader.frames()?,
        },
        KIND_SET_AUTOMATION_ENABLED => {
            let Some(address) = read_address(reader)? else {
                return Ok(Understanding::Newer);
            };
            OperationPayload::SetAutomationEnabled {
                address,
                enabled: reader.boolean()?,
            }
        }
        _ => return Ok(Understanding::Newer),
    };
    Ok(Understanding::Known(payload))
}

/// Rebuilds a parameter address, iteratively and within bounds.
///
/// `Ok(None)` means the bytes are well-formed but name something this build
/// cannot address — a newer owner, a newer key, a chain deeper than this build
/// permits, or a plugin identifier whose character set has since widened.
fn read_address(reader: &mut Reader<'_>) -> Result<Option<ParameterAddress>, WireError> {
    let mut owner = match reader.u8()? {
        OWNER_MASTER => ParameterOwner::Master,
        OWNER_LANE => ParameterOwner::Lane(reader.u32()?),
        OWNER_PLACEMENT => ParameterOwner::Placement(PlacementId::new(reader.u64()?)),
        _ => return Ok(None),
    };

    let depth = reader.u8()?;
    if usize::from(depth) > usize::from(ParameterOwner::MAX_DEPTH) {
        return Ok(None);
    }
    for _ in 0..depth {
        owner = owner.effect(reader.u8()?);
    }

    let key = match reader.u8()? {
        KEY_GAIN => ParameterKey::Gain,
        KEY_EQ_LOW => ParameterKey::EqLow,
        KEY_EQ_MID => ParameterKey::EqMid,
        KEY_EQ_HIGH => ParameterKey::EqHigh,
        KEY_FILTER => ParameterKey::Filter,
        KEY_MIX => ParameterKey::Mix,
        KEY_CROSSFADER => ParameterKey::Crossfader,
        KEY_PLUGIN => match PluginParameterId::new(reader.text()?) {
            Ok(id) => ParameterKey::Plugin(id),
            Err(_) => return Ok(None),
        },
        _ => return Ok(None),
    };

    Ok(ParameterAddress::new(owner, key).ok())
}

const fn marker_kind(code: u8) -> Option<MarkerKind> {
    match code {
        MARKER_CUE => Some(MarkerKind::Cue),
        MARKER_BUILD_UP => Some(MarkerKind::BuildUp),
        MARKER_DROP => Some(MarkerKind::Drop),
        MARKER_BREAKDOWN => Some(MarkerKind::Breakdown),
        MARKER_NOTE => Some(MarkerKind::Note),
        _ => None,
    }
}

const fn interpolation(code: u8) -> Option<Interpolation> {
    match code {
        INTERPOLATION_HOLD => Some(Interpolation::Hold),
        INTERPOLATION_LINEAR => Some(Interpolation::Linear),
        INTERPOLATION_SMOOTH => Some(Interpolation::Smooth),
        INTERPOLATION_ACCELERATING => Some(Interpolation::Accelerating),
        INTERPOLATION_DECELERATING => Some(Interpolation::Decelerating),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "a test that cannot build its own fixture should fail loudly"
    )]

    use super::*;

    fn device(id: u64) -> DeviceId {
        DeviceId::new(id)
    }

    fn address() -> ParameterAddress {
        ParameterAddress::new(
            ParameterOwner::Placement(PlacementId::new(7)).effect(2),
            ParameterKey::Filter,
        )
        .expect("valid address")
    }

    /// One of every payload, paired with the number it must always be given.
    ///
    /// The pairing is the point. A round trip alone would still pass if two
    /// variants swapped numbers in the same commit; pinning the number here
    /// means a shipped discriminant cannot quietly change its meaning, which is
    /// the promise ADR-0003 makes about a project written by an earlier build.
    fn every_payload() -> Vec<(u16, OperationPayload)> {
        vec![
            (
                KIND_SET_PROJECT_NAME,
                OperationPayload::SetProjectName {
                    name: "Friday, late".to_owned(),
                },
            ),
            (
                KIND_PLACE_TRACK,
                OperationPayload::PlaceTrack {
                    placement: PlacementId::new(1),
                    track: TrackRef::new(42),
                    position: Frames::new(48_000),
                    length: Frames::new(96_000),
                    lane: 3,
                },
            ),
            (
                KIND_MOVE_PLACEMENT,
                OperationPayload::MovePlacement {
                    placement: PlacementId::new(1),
                    position: Frames::new(-1),
                    lane: u32::MAX,
                },
            ),
            (
                KIND_TRIM_PLACEMENT,
                OperationPayload::TrimPlacement {
                    placement: PlacementId::new(2),
                    length: Frames::new(i64::MAX),
                },
            ),
            (
                KIND_REMOVE_PLACEMENT,
                OperationPayload::RemovePlacement {
                    placement: PlacementId::new(3),
                },
            ),
            (
                KIND_ADD_MARKER,
                OperationPayload::AddMarker {
                    marker: MarkerId::new(9),
                    position: Frames::new(1_234_567),
                    kind: MarkerKind::Drop,
                    label: "the one".to_owned(),
                },
            ),
            (
                KIND_REMOVE_MARKER,
                OperationPayload::RemoveMarker {
                    marker: MarkerId::new(9),
                },
            ),
            (
                KIND_SET_AUTOMATION_POINT,
                OperationPayload::SetAutomationPoint {
                    address: address(),
                    position: Frames::new(500),
                    value: 0.375,
                    interpolation: Interpolation::Smooth,
                },
            ),
            (
                KIND_REMOVE_AUTOMATION_POINT,
                OperationPayload::RemoveAutomationPoint {
                    address: address(),
                    position: Frames::new(500),
                },
            ),
            (
                KIND_SET_PLACEMENT_SOURCE,
                OperationPayload::SetPlacementSource {
                    placement: PlacementId::new(1),
                    source_offset: Frames::new(2_048),
                },
            ),
            (
                KIND_SET_TEMPO,
                OperationPayload::SetTempo {
                    position: Frames::ZERO,
                    tempo: Tempo::BPM_128,
                },
            ),
            (
                KIND_REMOVE_TEMPO,
                OperationPayload::RemoveTempo {
                    position: Frames::new(96_000),
                },
            ),
            (
                KIND_SET_AUTOMATION_ENABLED,
                OperationPayload::SetAutomationEnabled {
                    address: address(),
                    enabled: false,
                },
            ),
        ]
    }

    fn context() -> VersionVector {
        let mut vector = VersionVector::new();
        vector.observe(OperationId::new(device(2), 5));
        vector.observe(OperationId::new(device(1), 3));
        vector
    }

    fn operation(sequence: u64, payload: OperationPayload) -> Operation {
        Operation {
            id: OperationId::new(device(1), sequence),
            context: context(),
            timestamp_micros: 1_700_000_000_000_000,
            payload,
        }
    }

    fn all_operations() -> Vec<Operation> {
        every_payload()
            .into_iter()
            .enumerate()
            .map(|(index, (_, payload))| {
                let sequence = u64::try_from(index).expect("small") + 1;
                operation(sequence, payload)
            })
            .collect()
    }

    // ----- fixtures that build bytes by hand -------------------------------
    //
    // Everything hostile, and everything from a build that does not exist yet,
    // has to be constructed rather than encoded — the encoder cannot produce it,
    // which is exactly why the decoder has to be tested against it.

    fn u32_of(value: usize) -> u32 {
        u32::try_from(value).expect("fits")
    }

    fn message_of(entries: &[Vec<u8>]) -> Vec<u8> {
        header_of(entries.len(), FORMAT_MINOR, &[], entries)
    }

    fn header_of(count: usize, minor: u16, extra: &[u8], entries: &[Vec<u8>]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&MAGIC);
        put_u16(&mut out, FORMAT_MAJOR);
        put_u16(&mut out, minor);
        put_u32(&mut out, u32_of(4 + extra.len()));
        put_u32(&mut out, u32_of(count));
        out.extend_from_slice(extra);
        for entry in entries {
            put_u32(&mut out, u32_of(entry.len()));
            out.extend_from_slice(entry);
        }
        out
    }

    fn entry_of(sequence: u64, kind: u16, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        put_u64(&mut out, 1);
        put_u64(&mut out, sequence);
        put_i64(&mut out, 1_700_000_000_000_000);
        put_u32(&mut out, 0);
        put_u16(&mut out, kind);
        put_u32(&mut out, u32_of(payload.len()));
        out.extend_from_slice(payload);
        out
    }

    // ----- the journey ------------------------------------------------------

    #[test]
    fn every_payload_survives_the_journey_intact() {
        let operations = all_operations();
        let bytes = encode(&operations).expect("encodes");
        let message = decode(&bytes).expect("decodes");

        assert_eq!(message.len(), operations.len());
        assert_eq!(message.unrecognised_count(), 0);
        assert_eq!(message.to_operations(), operations);
        assert_eq!(message.minor(), FORMAT_MINOR);
    }

    #[test]
    fn every_payload_keeps_the_number_it_was_given() {
        for (kind, payload) in every_payload() {
            let mut body = Vec::new();
            let written = write_payload(&mut body, &payload).expect("encodes");
            assert_eq!(written, kind, "{payload:?} changed its discriminant");
        }
    }

    #[test]
    fn no_two_payloads_share_a_number() {
        let mut seen = Vec::new();
        for (kind, _) in every_payload() {
            assert!(
                !seen.contains(&kind),
                "two payloads share the number {kind}"
            );
            seen.push(kind);
        }
        // Every number from one upward is accounted for, so a new variant takes
        // the next one rather than a gap left by something withdrawn — which
        // ADR-0003 does not permit in the first place.
        seen.sort_unstable();
        let expected: Vec<u16> = (1..=u16::try_from(seen.len()).expect("small")).collect();
        assert_eq!(seen, expected);
    }

    #[test]
    fn the_same_operations_always_produce_the_same_bytes() {
        // Canonical encoding. The version vector is a map, and a map with an
        // unspecified order would make two devices holding identical state
        // produce different bytes — which breaks comparison, caching and any
        // signature over the message.
        let mut one = VersionVector::new();
        one.observe(OperationId::new(device(9), 2));
        one.observe(OperationId::new(device(4), 7));
        one.observe(OperationId::new(device(6), 1));

        let mut other = VersionVector::new();
        other.observe(OperationId::new(device(6), 1));
        other.observe(OperationId::new(device(9), 2));
        other.observe(OperationId::new(device(4), 7));

        let payload = OperationPayload::RemoveMarker {
            marker: MarkerId::new(1),
        };
        let build = |context: VersionVector| {
            encode(&[Operation {
                id: OperationId::new(device(1), 1),
                context,
                timestamp_micros: 0,
                payload: payload.clone(),
            }])
            .expect("encodes")
        };

        assert_eq!(build(one), build(other));
    }

    #[test]
    fn a_round_trip_reproduces_the_bytes_it_was_given() {
        let bytes = encode(&all_operations()).expect("encodes");
        let message = decode(&bytes).expect("decodes");
        assert_eq!(message.encode().expect("re-encodes"), bytes);
    }

    #[test]
    fn an_empty_message_is_a_message() {
        let bytes = encode(&[]).expect("encodes");
        let message = decode(&bytes).expect("decodes");
        assert!(message.is_empty());
        assert_eq!(message.encode().expect("re-encodes"), bytes);
    }

    // ----- carrying what cannot be understood -------------------------------

    #[test]
    fn an_operation_from_a_newer_build_is_carried_rather_than_dropped() {
        // The property the whole format is shaped around: a device running last
        // year's build is a relay, not a hole in the fleet.
        let unknown = entry_of(1, 60_000, b"whatever this means");
        let known = {
            let mut entry = Vec::new();
            let mut body = Vec::new();
            write_operation(
                &mut entry,
                &mut body,
                &operation(
                    2,
                    OperationPayload::RemoveMarker {
                        marker: MarkerId::new(1),
                    },
                ),
            )
            .expect("encodes");
            entry
        };
        let bytes = message_of(&[unknown, known]);

        let message = decode(&bytes).expect("decodes");
        assert_eq!(message.len(), 2);
        assert_eq!(message.understood().count(), 1);
        assert_eq!(message.unrecognised_count(), 1);

        let carried = message.unrecognised().next().expect("one carried");
        assert_eq!(carried.kind(), 60_000);
        assert_eq!(carried.id(), OperationId::new(device(1), 1));
        assert_eq!(carried.timestamp_micros(), 1_700_000_000_000_000);
        assert_eq!(carried.context(), &VersionVector::new());
        assert!(carried.byte_len() > 0);

        // And passing it on does not alter a byte of it.
        assert_eq!(message.encode().expect("re-encodes"), bytes);
    }

    #[test]
    fn an_entry_a_newer_build_extended_is_not_half_applied() {
        // The payload is one this build knows. The entry carries something after
        // it that this build does not. Applying the part we recognise would
        // store an edit stripped of whatever the rest meant, and then relay our
        // stripped version as though it were the author's.
        let mut entry = Vec::new();
        let mut body = Vec::new();
        write_operation(
            &mut entry,
            &mut body,
            &operation(
                1,
                OperationPayload::RemoveMarker {
                    marker: MarkerId::new(1),
                },
            ),
        )
        .expect("encodes");
        entry.extend_from_slice(b"a field from next year");

        let bytes = message_of(&[entry]);
        let message = decode(&bytes).expect("decodes");

        assert_eq!(message.understood().count(), 0);
        assert_eq!(message.unrecognised_count(), 1);
        // The kind was not the unfamiliar part, and saying otherwise would send
        // whoever reads the report looking for a payload number that is fine.
        assert_eq!(message.unrecognised().next().expect("one").kind(), 0);
        assert_eq!(message.encode().expect("re-encodes"), bytes);
    }

    #[test]
    fn a_payload_a_newer_build_extended_is_not_half_applied() {
        // The same situation one level down: a field appended inside a payload
        // this build knows.
        let mut payload = Vec::new();
        put_u64(&mut payload, 9);
        payload.push(0xAB);

        let bytes = message_of(&[entry_of(1, KIND_REMOVE_MARKER, &payload)]);
        let message = decode(&bytes).expect("decodes");

        assert_eq!(message.understood().count(), 0);
        assert_eq!(message.unrecognised_count(), 1);
        assert_eq!(message.encode().expect("re-encodes"), bytes);
    }

    #[test]
    fn a_newer_value_inside_a_known_payload_travels_rather_than_failing() {
        // A marker kind, an interpolation shape and a parameter key are all
        // `non_exhaustive`: they will grow. A reader that rejected the message
        // would make one new marker kind break synchronisation entirely.
        let mut payload = Vec::new();
        put_u64(&mut payload, 9);
        put_i64(&mut payload, 0);
        payload.push(200);
        put_u32(&mut payload, 0);

        let bytes = message_of(&[entry_of(1, KIND_ADD_MARKER, &payload)]);
        let message = decode(&bytes).expect("a newer marker kind failed the message");

        assert_eq!(message.unrecognised_count(), 1);
        assert_eq!(
            message.unrecognised().next().expect("one").kind(),
            KIND_ADD_MARKER
        );
        assert_eq!(message.encode().expect("re-encodes"), bytes);
    }

    #[test]
    fn a_tempo_beyond_this_builds_range_travels_rather_than_failing() {
        let mut payload = Vec::new();
        put_i64(&mut payload, 0);
        put_u64(&mut payload, 1);

        let bytes = message_of(&[entry_of(1, KIND_SET_TEMPO, &payload)]);
        let message = decode(&bytes).expect("decodes");
        assert_eq!(message.unrecognised_count(), 1);
    }

    #[test]
    fn a_version_vector_travels_on_its_own() {
        // The half of the protocol that goes first: a device says what it has
        // seen, and the other side answers with what it has not.
        let vector = context();
        let bytes = encode_vector(&vector).expect("encodes");
        assert_eq!(decode_vector(&bytes).expect("decodes"), vector);

        let empty = VersionVector::new();
        let bytes = encode_vector(&empty).expect("encodes");
        assert_eq!(decode_vector(&bytes).expect("decodes"), empty);
    }

    #[test]
    fn a_vector_and_a_log_are_never_mistaken_for_one_another() {
        // Read the wrong way round, a vector would look like a log carrying no
        // operations — a device that had nothing to send, which is exactly what
        // a synchronisation that silently does nothing looks like.
        let operations = all_operations();
        let log = encode(&operations).expect("encodes");
        let vector = encode_vector(&context()).expect("encodes");

        assert_eq!(decode(&vector), Err(WireError::NotAMessage));
        assert_eq!(decode_vector(&log), Err(WireError::NotAMessage));
    }

    #[test]
    fn a_vector_from_a_newer_build_is_read_as_far_as_it_goes() {
        let mut body = Vec::new();
        put_u32(&mut body, 1);
        put_u64(&mut body, 3);
        put_u64(&mut body, 9);
        body.extend_from_slice(b"and something else besides");

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&VECTOR_MAGIC);
        put_u16(&mut bytes, FORMAT_MAJOR);
        put_u16(&mut bytes, 42);
        put_u32(&mut bytes, u32_of(body.len()));
        bytes.extend_from_slice(&body);

        let vector = decode_vector(&bytes).expect("a newer vector was refused");
        assert_eq!(vector.sequence_for(device(3)), 9);
    }

    #[test]
    fn a_truncated_vector_is_refused_at_every_length() {
        let bytes = encode_vector(&context()).expect("encodes");
        for length in 0..bytes.len() {
            assert!(
                decode_vector(&bytes[..length]).is_err(),
                "{length} bytes of a {} byte vector decoded",
                bytes.len()
            );
        }
        assert!(decode_vector(&bytes).is_ok());
    }

    #[test]
    fn a_header_field_from_a_newer_build_is_stepped_over_and_kept() {
        // What makes the minor version mean anything. Without a length around
        // the header, a reader meeting an added field would have no way to find
        // where the entries begin, and "additive" would mean "unreadable".
        let entry = entry_of(1, KIND_REMOVE_TEMPO, &0_i64.to_le_bytes());
        let bytes = header_of(1, 7, b"something about the message", &[entry]);

        let message = decode(&bytes).expect("an added header field broke the parse");
        assert_eq!(message.minor(), 7);
        assert_eq!(message.understood().count(), 1);
        assert!(message.carries_unread_header_fields());

        // And it survives being passed on, along with the version that
        // announced it. A relay that restamped either would be claiming the
        // message was older and plainer than it is.
        assert_eq!(message.encode().expect("re-encodes"), bytes);
    }

    #[test]
    fn a_message_this_build_wrote_claims_nothing_it_did_not_add() {
        let bytes = encode(&all_operations()).expect("encodes");
        let message = decode(&bytes).expect("decodes");
        assert_eq!(message.minor(), FORMAT_MINOR);
        assert!(!message.carries_unread_header_fields());
    }

    #[test]
    fn a_higher_minor_version_is_read_and_a_higher_major_is_not() {
        let mut bytes = encode(&all_operations()).expect("encodes");
        bytes[6] = 200;
        let message = decode(&bytes).expect("a newer minor version was refused");
        assert_eq!(message.minor(), 200);
        // And the minor version it was given travels with it, so a relay does
        // not claim work is older than it is.
        assert_eq!(message.understood().count(), all_operations().len());

        bytes[4] = FORMAT_MAJOR.wrapping_add(1).to_le_bytes()[0];
        assert!(matches!(
            decode(&bytes),
            Err(WireError::UnsupportedVersion { .. })
        ));
    }

    // ----- what no build ever meant -----------------------------------------

    #[test]
    fn something_that_is_not_a_message_is_refused_on_the_first_read() {
        assert_eq!(decode(b"GIF89a...."), Err(WireError::NotAMessage));
        assert_eq!(decode(b""), Err(WireError::Truncated));
    }

    #[test]
    fn truncation_at_every_length_is_refused_rather_than_accepted() {
        let bytes = encode(&all_operations()).expect("encodes");
        for length in 0..bytes.len() {
            let result = decode(&bytes[..length]);
            assert!(
                result.is_err(),
                "{length} bytes of a {} byte message decoded",
                bytes.len()
            );
        }
        assert!(decode(&bytes).is_ok());
    }

    #[test]
    fn bytes_after_the_last_operation_are_refused() {
        let mut bytes = encode(&all_operations()).expect("encodes");
        bytes.push(0);
        assert!(matches!(
            decode(&bytes),
            Err(WireError::TrailingBytes { .. })
        ));
    }

    #[test]
    fn a_count_larger_than_the_bytes_present_is_refused_without_allocating() {
        // The guard that matters most. A twelve-byte message claiming four
        // billion operations must not persuade a reader to reserve room for
        // them — and the check is not a fixed limit but the arithmetic: four
        // billion operations cannot fit in twelve bytes.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&MAGIC);
        put_u16(&mut bytes, FORMAT_MAJOR);
        put_u16(&mut bytes, FORMAT_MINOR);
        put_u32(&mut bytes, 4);
        put_u32(&mut bytes, u32::MAX);

        assert!(matches!(
            decode(&bytes),
            Err(WireError::ImplausibleCount { .. })
        ));

        // The same claim inside an entry: a version vector of four billion
        // devices in an entry that carries none of them.
        let mut entry = Vec::new();
        put_u64(&mut entry, 1);
        put_u64(&mut entry, 1);
        put_i64(&mut entry, 0);
        put_u32(&mut entry, u32::MAX);
        assert!(matches!(
            decode(&message_of(&[entry])),
            Err(WireError::ImplausibleCount { .. })
        ));

        // And inside a string.
        let mut payload = Vec::new();
        put_u32(&mut payload, u32::MAX);
        assert!(matches!(
            decode(&message_of(&[entry_of(1, KIND_SET_PROJECT_NAME, &payload)])),
            Err(WireError::ImplausibleCount { .. })
        ));
    }

    #[test]
    fn text_that_is_not_utf8_is_refused() {
        let mut payload = Vec::new();
        put_u32(&mut payload, 2);
        payload.extend_from_slice(&[0xFF, 0xFE]);
        assert_eq!(
            decode(&message_of(&[entry_of(1, KIND_SET_PROJECT_NAME, &payload)])),
            Err(WireError::InvalidText)
        );
    }

    #[test]
    fn a_boolean_that_is_neither_true_nor_false_is_refused() {
        let mut payload = Vec::new();
        put_address(&mut payload, &address()).expect("encodes");
        payload.push(2);
        assert_eq!(
            decode(&message_of(&[entry_of(
                1,
                KIND_SET_AUTOMATION_ENABLED,
                &payload
            )])),
            Err(WireError::InvalidBoolean { value: 2 })
        );
    }

    #[test]
    fn a_value_that_is_not_a_number_is_refused_at_both_ends() {
        // Sending one would be a bug in this build; receiving one would put a
        // value into an automation lane that no arithmetic recovers from.
        let sent = encode(&[operation(
            1,
            OperationPayload::SetAutomationPoint {
                address: address(),
                position: Frames::ZERO,
                value: f32::NAN,
                interpolation: Interpolation::Linear,
            },
        )]);
        assert_eq!(sent, Err(WireError::ValueNotFinite));

        let mut payload = Vec::new();
        put_address(&mut payload, &address()).expect("encodes");
        put_i64(&mut payload, 0);
        payload.extend_from_slice(&f32::INFINITY.to_le_bytes());
        payload.push(INTERPOLATION_LINEAR);
        assert_eq!(
            decode(&message_of(&[entry_of(
                1,
                KIND_SET_AUTOMATION_POINT,
                &payload
            )])),
            Err(WireError::ValueNotFinite)
        );
    }

    #[test]
    fn an_operation_numbered_zero_is_refused() {
        // No device issues one: sequences start at one. A zero would be seen as
        // already present by every version vector, so the edit would vanish
        // silently — which is the one outcome this system does not permit.
        assert!(matches!(
            decode(&message_of(&[entry_of(0, KIND_REMOVE_TEMPO, &[0; 8])])),
            Err(WireError::InvalidSequence { .. })
        ));
    }

    #[test]
    fn a_zeroed_buffer_is_never_mistaken_for_an_edit() {
        // Every tag in the format starts at one, so a buffer that was zeroed, or
        // truncated and padded, cannot decode into a plausible-looking address.
        let mut payload = Vec::new();
        payload.extend_from_slice(&[0; 32]);
        let message = decode(&message_of(&[entry_of(
            1,
            KIND_SET_AUTOMATION_POINT,
            &payload,
        )]))
        .expect("decodes");
        assert_eq!(message.understood().count(), 0);
        assert_eq!(message.unrecognised_count(), 1);
    }

    #[test]
    fn a_chain_deeper_than_this_build_addresses_is_refused_without_recursion() {
        // A tree decoded by recursion would recurse as deeply as the sender
        // asked. The chain is written flat and bounded by a byte, and the bound
        // is checked before anything is built.
        let mut payload = Vec::new();
        payload.push(OWNER_MASTER);
        payload.push(u8::MAX);
        for slot in 0..u8::MAX {
            payload.push(slot);
        }
        payload.push(KEY_GAIN);
        put_i64(&mut payload, 0);

        let message = decode(&message_of(&[entry_of(
            1,
            KIND_REMOVE_AUTOMATION_POINT,
            &payload,
        )]))
        .expect("decodes");
        assert_eq!(message.unrecognised_count(), 1);
    }

    #[test]
    fn text_beyond_the_limit_is_refused_where_someone_can_still_be_told() {
        let long = "x".repeat(MAX_TEXT_BYTES + 1);
        assert_eq!(
            encode(&[operation(
                1,
                OperationPayload::SetProjectName { name: long }
            )]),
            Err(WireError::TextTooLong {
                length: MAX_TEXT_BYTES + 1,
                maximum: MAX_TEXT_BYTES,
            })
        );

        let allowed = "x".repeat(MAX_TEXT_BYTES);
        assert!(encode(&[operation(
            1,
            OperationPayload::SetProjectName { name: allowed }
        )])
        .is_ok());
    }

    #[test]
    fn no_single_altered_byte_makes_a_reader_misbehave() {
        // What this does *not* claim is that a damaged message is detected.
        // Nothing here authenticates the bytes, so altering the device field
        // yields a valid operation attributed to another device — and no format
        // without a shared secret can tell that from the truth. That is why the
        // module says integrity is the transport's job, and this test is where
        // the limit of the claim is written down rather than assumed.
        //
        // What it does establish is that a reader cannot be made to misbehave.
        // Every damaged message is refused, or decoded into something this
        // build can write out and read back identically — a fixed point. Never
        // a panic, never an allocation the bytes did not pay for, never an
        // entry that changes on its way through.
        //
        // A fixed point rather than the original bytes, because a message need
        // not arrive canonical: a version vector listed out of device order
        // decodes to the same map and is written back in order. Nothing is lost
        // — the peer sent a map and a map is what travels on — but the bytes
        // are this build's rendering of it rather than a copy.
        let original = encode(&all_operations()).expect("encodes");

        for index in 0..original.len() {
            for replacement in [0x00, 0x01, 0x7F, 0x80, 0xFF] {
                let mut damaged = original.clone();
                if damaged[index] == replacement {
                    continue;
                }
                damaged[index] = replacement;
                let Ok(message) = decode(&damaged) else {
                    continue;
                };
                let written = message.encode().expect("re-encodes");
                assert_eq!(
                    decode(&written).as_ref(),
                    Ok(&message),
                    "byte {index} set to {replacement:#04x} decoded into something that \
                     does not survive being written out"
                );
                // Never larger than what arrived. A relay that could be made to
                // emit more than it received would let a fleet of them inflate
                // one message without bound. Smaller is permitted and does
                // happen: a vector naming the same device twice keeps the
                // higher sequence, which is what the vector means.
                assert!(
                    written.len() <= damaged.len(),
                    "byte {index} set to {replacement:#04x} grew from {} to {} bytes",
                    damaged.len(),
                    written.len()
                );
            }
        }
    }

    #[test]
    fn a_vector_that_arrives_out_of_order_is_normalised_without_losing_anything() {
        // A peer is not obliged to write its version vector in device order,
        // and this build will not reject one that does not. What it will do is
        // write it back out canonically, because a canonical encoding is what
        // makes two devices holding the same state produce the same bytes.
        let mut payload = Vec::new();
        put_i64(&mut payload, 0);

        let mut entry = Vec::new();
        put_u64(&mut entry, 1);
        put_u64(&mut entry, 1);
        put_i64(&mut entry, 0);
        put_u32(&mut entry, 2);
        put_u64(&mut entry, 9);
        put_u64(&mut entry, 4);
        put_u64(&mut entry, 2);
        put_u64(&mut entry, 8);
        put_u16(&mut entry, KIND_REMOVE_TEMPO);
        put_u32(&mut entry, u32_of(payload.len()));
        entry.extend_from_slice(&payload);

        let message = decode(&message_of(&[entry])).expect("decodes");
        let context = message.understood().next().expect("one").context.clone();
        assert_eq!(context.sequence_for(device(9)), 4);
        assert_eq!(context.sequence_for(device(2)), 8);

        let written = message.encode().expect("re-encodes");
        assert_eq!(decode(&written).expect("decodes"), message);
    }

    #[test]
    fn a_message_a_build_cannot_read_still_arrives_at_the_build_that_can() {
        // Three devices, the middle one a version behind. What it cannot apply
        // it still carries, so the third device receives the second's work
        // intact — which is the difference between a stale install being
        // inconvenient and being a hole in the fleet.
        let newer = message_of(&[
            entry_of(1, 60_000, b"an edit from next year"),
            entry_of(2, KIND_REMOVE_TEMPO, &0_i64.to_le_bytes()),
        ]);

        let relayed = decode(&newer).expect("the stale build reads what it can");
        assert_eq!(relayed.understood().count(), 1);
        assert_eq!(relayed.unrecognised_count(), 1);
        let forwarded = relayed.encode().expect("passes it on");

        assert_eq!(forwarded, newer);
        let arrived = decode(&forwarded).expect("decodes");
        assert_eq!(arrived.len(), 2);
        assert_eq!(
            arrived.unrecognised().next().expect("one").kind(),
            60_000,
            "the relay altered what it was carrying"
        );
    }
}

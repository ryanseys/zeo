//! `Encoding::Converter` (CRuby transcode.c) -- the stateful, chunk-at-a-time
//! face of the same engine `String#encode` runs on.
//!
//! The difference from `String#encode` is only WHERE the conversion can stop.
//! `enc::transcode_run` reports how far it got and why; this class keeps the
//! leftovers ([`ConvState::pending`]), the escape-mode state, and the last
//! refusal, so a caller can feed it a stream one piece at a time.
//!
//! `#convpath` is a real graph search. The 194 direct converters CRuby
//! registers are tabulated in [`EDGES`] (derived from the oracle by asking
//! `search_convpath` for all 10,609 encoding pairs), and a breadth-first walk
//! over them reproduces every one of those answers exactly.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::builtins::encoding::{arg_encoding, encoding_value};
use crate::builtins::{arg_error, check_arity, convert, type_error};
use crate::dispatch::{ClassRegistry, RObj, RubyObject};
use crate::encoding::{
    self, EncodingId, NewlineMode, Stop, TranscodeOptions, TranscodeState, XmlMode,
};
use crate::{RubyValue, Signal};
use zeo_abi::{ClassId, ENCODING_CONVERTER_CLASS};
use zeo_macros::ruby_class;

/// The option-mask constants, in the order `Encoding::Converter.constants`
/// answers them. Every one is a plain Integer on the class.
const CONSTANTS: &[(&str, i64)] = &[
    ("INVALID_MASK", 0x0000_000F),
    ("INVALID_REPLACE", 0x0000_0002),
    ("UNDEF_MASK", 0x0000_00F0),
    ("UNDEF_REPLACE", 0x0000_0020),
    ("UNDEF_HEX_CHARREF", 0x0000_0030),
    ("UNIVERSAL_NEWLINE_DECORATOR", 0x0000_0100),
    ("CRLF_NEWLINE_DECORATOR", 0x0000_1000),
    ("CR_NEWLINE_DECORATOR", 0x0000_2000),
    ("LF_NEWLINE_DECORATOR", 0x0000_4000),
    ("XML_TEXT_DECORATOR", 0x0000_8000),
    ("XML_ATTR_CONTENT_DECORATOR", 0x0001_0000),
    ("PARTIAL_INPUT", 0x0002_0000),
    ("AFTER_OUTPUT", 0x0004_0000),
    ("XML_ATTR_QUOTE_DECORATOR", 0x0010_0000),
];

const INVALID_REPLACE: i64 = 0x0000_0002;
const UNDEF_REPLACE: i64 = 0x0000_0020;
const UNDEF_HEX_CHARREF: i64 = 0x0000_0030;
const UNIVERSAL_NEWLINE: i64 = 0x0000_0100;
const CRLF_NEWLINE: i64 = 0x0000_1000;
const CR_NEWLINE: i64 = 0x0000_2000;
const XML_TEXT: i64 = 0x0000_8000;
const XML_ATTR_CONTENT: i64 = 0x0001_0000;
const XML_ATTR_QUOTE: i64 = 0x0010_0000;

/// Each decorator flag with the two names CRuby spells it under: the one
/// `#convpath` lists and the one `#inspect` and the not-found message use.
/// In ASCENDING FLAG ORDER, which is the order the message lists them in.
const DECORATORS: &[(i64, &str, &str)] = &[
    (UNIVERSAL_NEWLINE, "universal_newline", "universal_newline"),
    (CRLF_NEWLINE, "crlf_newline", "crlf_newline"),
    (CR_NEWLINE, "cr_newline", "cr_newline"),
    (XML_TEXT, "xml_text_escape", "xml_text"),
    (
        XML_ATTR_CONTENT,
        "xml_attr_content_escape",
        "xml_attr_content",
    ),
    (XML_ATTR_QUOTE, "xml_attr_quote", "xml_attr_quote"),
];

/// The direct converters CRuby registers, `"source>destination"`. GENERATED
/// from the ruby 4.0.6 oracle: every consecutive pair that appears in any
/// `Encoding::Converter.search_convpath` answer over all 103 encodings. A
/// breadth-first walk over these reproduces all 10,609 of those answers.
const EDGES: &[(&str, &str)] = &[
    ("ASCII-8BIT", "UTF-8"),
    ("Big5-HKSCS", "UTF-8"),
    ("Big5-UAO", "UTF-8"),
    ("Big5", "UTF-8"),
    ("CESU-8", "UTF-8"),
    ("CP50220", "CP51932"),
    ("CP50221", "CP51932"),
    ("CP51932", "CP50220"),
    ("CP51932", "CP50221"),
    ("CP51932", "UTF-8"),
    ("CP850", "UTF-8"),
    ("CP852", "UTF-8"),
    ("CP855", "UTF-8"),
    ("CP949", "UTF-8"),
    ("CP950", "UTF-8"),
    ("CP951", "UTF-8"),
    ("EUC-JIS-2004", "UTF-8"),
    ("EUC-JP", "Shift_JIS"),
    ("EUC-JP", "UTF-8"),
    ("EUC-JP", "stateless-ISO-2022-JP"),
    ("EUC-KR", "UTF-8"),
    ("GB12345", "UTF-8"),
    ("GB18030", "UTF-8"),
    ("GB2312", "UTF-8"),
    ("GBK", "UTF-8"),
    ("IBM037", "ISO-8859-1"),
    ("IBM437", "UTF-8"),
    ("IBM720", "UTF-8"),
    ("IBM737", "UTF-8"),
    ("IBM775", "UTF-8"),
    ("IBM852", "UTF-8"),
    ("IBM855", "UTF-8"),
    ("IBM857", "UTF-8"),
    ("IBM860", "UTF-8"),
    ("IBM861", "UTF-8"),
    ("IBM862", "UTF-8"),
    ("IBM863", "UTF-8"),
    ("IBM864", "UTF-8"),
    ("IBM865", "UTF-8"),
    ("IBM866", "UTF-8"),
    ("IBM869", "UTF-8"),
    ("ISO-2022-JP-KDDI", "stateless-ISO-2022-JP-KDDI"),
    ("ISO-2022-JP", "stateless-ISO-2022-JP"),
    ("ISO-8859-10", "UTF-8"),
    ("ISO-8859-11", "UTF-8"),
    ("ISO-8859-13", "UTF-8"),
    ("ISO-8859-14", "UTF-8"),
    ("ISO-8859-15", "UTF-8"),
    ("ISO-8859-16", "UTF-8"),
    ("ISO-8859-1", "IBM037"),
    ("ISO-8859-1", "UTF-8"),
    ("ISO-8859-2", "UTF-8"),
    ("ISO-8859-3", "UTF-8"),
    ("ISO-8859-4", "UTF-8"),
    ("ISO-8859-5", "UTF-8"),
    ("ISO-8859-6", "UTF-8"),
    ("ISO-8859-7", "UTF-8"),
    ("ISO-8859-8", "UTF-8"),
    ("ISO-8859-9", "UTF-8"),
    ("KOI8-R", "UTF-8"),
    ("KOI8-U", "UTF-8"),
    ("SJIS-DoCoMo", "UTF8-DoCoMo"),
    ("SJIS-KDDI", "UTF8-KDDI"),
    ("SJIS-SoftBank", "UTF8-SoftBank"),
    ("Shift_JIS", "EUC-JP"),
    ("Shift_JIS", "UTF-8"),
    ("TIS-620", "UTF-8"),
    ("US-ASCII", "UTF-8"),
    ("UTF-16", "UTF-8"),
    ("UTF-16BE", "UTF-8"),
    ("UTF-16LE", "UTF-8"),
    ("UTF-32", "UTF-8"),
    ("UTF-32BE", "UTF-8"),
    ("UTF-32LE", "UTF-8"),
    ("UTF-8", "ASCII-8BIT"),
    ("UTF-8", "Big5"),
    ("UTF-8", "Big5-HKSCS"),
    ("UTF-8", "Big5-UAO"),
    ("UTF-8", "CESU-8"),
    ("UTF-8", "CP51932"),
    ("UTF-8", "CP850"),
    ("UTF-8", "CP852"),
    ("UTF-8", "CP855"),
    ("UTF-8", "CP949"),
    ("UTF-8", "CP950"),
    ("UTF-8", "CP951"),
    ("UTF-8", "EUC-JIS-2004"),
    ("UTF-8", "EUC-JP"),
    ("UTF-8", "EUC-KR"),
    ("UTF-8", "GB12345"),
    ("UTF-8", "GB18030"),
    ("UTF-8", "GB2312"),
    ("UTF-8", "GBK"),
    ("UTF-8", "IBM437"),
    ("UTF-8", "IBM720"),
    ("UTF-8", "IBM737"),
    ("UTF-8", "IBM775"),
    ("UTF-8", "IBM852"),
    ("UTF-8", "IBM855"),
    ("UTF-8", "IBM857"),
    ("UTF-8", "IBM860"),
    ("UTF-8", "IBM861"),
    ("UTF-8", "IBM862"),
    ("UTF-8", "IBM863"),
    ("UTF-8", "IBM864"),
    ("UTF-8", "IBM865"),
    ("UTF-8", "IBM866"),
    ("UTF-8", "IBM869"),
    ("UTF-8", "ISO-8859-1"),
    ("UTF-8", "ISO-8859-10"),
    ("UTF-8", "ISO-8859-11"),
    ("UTF-8", "ISO-8859-13"),
    ("UTF-8", "ISO-8859-14"),
    ("UTF-8", "ISO-8859-15"),
    ("UTF-8", "ISO-8859-16"),
    ("UTF-8", "ISO-8859-2"),
    ("UTF-8", "ISO-8859-3"),
    ("UTF-8", "ISO-8859-4"),
    ("UTF-8", "ISO-8859-5"),
    ("UTF-8", "ISO-8859-6"),
    ("UTF-8", "ISO-8859-7"),
    ("UTF-8", "ISO-8859-8"),
    ("UTF-8", "ISO-8859-9"),
    ("UTF-8", "KOI8-R"),
    ("UTF-8", "KOI8-U"),
    ("UTF-8", "Shift_JIS"),
    ("UTF-8", "TIS-620"),
    ("UTF-8", "US-ASCII"),
    ("UTF-8", "UTF-16"),
    ("UTF-8", "UTF-16BE"),
    ("UTF-8", "UTF-16LE"),
    ("UTF-8", "UTF-32"),
    ("UTF-8", "UTF-32BE"),
    ("UTF-8", "UTF-32LE"),
    ("UTF-8", "UTF8-DoCoMo"),
    ("UTF-8", "UTF8-KDDI"),
    ("UTF-8", "UTF8-MAC"),
    ("UTF-8", "UTF8-SoftBank"),
    ("UTF-8", "Windows-1250"),
    ("UTF-8", "Windows-1251"),
    ("UTF-8", "Windows-1252"),
    ("UTF-8", "Windows-1253"),
    ("UTF-8", "Windows-1254"),
    ("UTF-8", "Windows-1255"),
    ("UTF-8", "Windows-1256"),
    ("UTF-8", "Windows-1257"),
    ("UTF-8", "Windows-31J"),
    ("UTF-8", "Windows-874"),
    ("UTF-8", "eucJP-ms"),
    ("UTF-8", "macCroatian"),
    ("UTF-8", "macCyrillic"),
    ("UTF-8", "macGreek"),
    ("UTF-8", "macIceland"),
    ("UTF-8", "macRoman"),
    ("UTF-8", "macRomania"),
    ("UTF-8", "macTurkish"),
    ("UTF-8", "macUkraine"),
    ("UTF8-DoCoMo", "SJIS-DoCoMo"),
    ("UTF8-DoCoMo", "UTF-8"),
    ("UTF8-DoCoMo", "UTF8-KDDI"),
    ("UTF8-DoCoMo", "UTF8-SoftBank"),
    ("UTF8-KDDI", "SJIS-KDDI"),
    ("UTF8-KDDI", "UTF-8"),
    ("UTF8-KDDI", "UTF8-DoCoMo"),
    ("UTF8-KDDI", "UTF8-SoftBank"),
    ("UTF8-KDDI", "stateless-ISO-2022-JP-KDDI"),
    ("UTF8-MAC", "UTF-8"),
    ("UTF8-SoftBank", "SJIS-SoftBank"),
    ("UTF8-SoftBank", "UTF-8"),
    ("UTF8-SoftBank", "UTF8-DoCoMo"),
    ("UTF8-SoftBank", "UTF8-KDDI"),
    ("Windows-1250", "UTF-8"),
    ("Windows-1251", "UTF-8"),
    ("Windows-1252", "UTF-8"),
    ("Windows-1253", "UTF-8"),
    ("Windows-1254", "UTF-8"),
    ("Windows-1255", "UTF-8"),
    ("Windows-1256", "UTF-8"),
    ("Windows-1257", "UTF-8"),
    ("Windows-31J", "UTF-8"),
    ("Windows-874", "UTF-8"),
    ("eucJP-ms", "UTF-8"),
    ("macCroatian", "UTF-8"),
    ("macCyrillic", "UTF-8"),
    ("macGreek", "UTF-8"),
    ("macIceland", "UTF-8"),
    ("macRoman", "UTF-8"),
    ("macRomania", "UTF-8"),
    ("macTurkish", "UTF-8"),
    ("macUkraine", "UTF-8"),
    ("stateless-ISO-2022-JP-KDDI", "ISO-2022-JP-KDDI"),
    ("stateless-ISO-2022-JP-KDDI", "UTF8-KDDI"),
    ("stateless-ISO-2022-JP", "EUC-JP"),
    ("stateless-ISO-2022-JP", "ISO-2022-JP"),
];

/// `Encoding::Converter.asciicompat_encoding`: the ascii-compatible encoding
/// a converter reads this one THROUGH. A per-encoding fact rather than a
/// graph one -- `UTF8-MAC` also has exactly one outgoing edge and answers
/// nil -- so it is tabulated straight from the oracle. Every encoding absent
/// here answers nil.
const ASCII_COMPAT: &[(&str, &str)] = &[
    ("UTF-16BE", "UTF-8"),
    ("UTF-16LE", "UTF-8"),
    ("UTF-32BE", "UTF-8"),
    ("UTF-32LE", "UTF-8"),
    ("UTF-16", "UTF-8"),
    ("UTF-32", "UTF-8"),
    ("CESU-8", "UTF-8"),
    ("IBM037", "ISO-8859-1"),
    ("ISO-2022-JP", "stateless-ISO-2022-JP"),
    ("CP50220", "CP51932"),
    ("CP50221", "CP51932"),
    ("ISO-2022-JP-KDDI", "stateless-ISO-2022-JP-KDDI"),
];

/// The shortest converter path from `src` to `dst`, as encoding-name hops.
/// `None` when no chain of registered converters joins them -- which
/// includes every same-encoding pair, since CRuby registers no converter
/// from an encoding to itself.
fn convpath(src: EncodingId, dst: EncodingId) -> Option<Vec<(&'static str, &'static str)>> {
    if src == dst {
        return None;
    }
    let (from, to) = (src.name(), dst.name());
    let mut queue: Vec<Vec<&'static str>> = vec![vec![from]];
    let mut seen: Vec<&str> = vec![from];
    let mut head = 0;
    while head < queue.len() {
        let path = queue[head].clone();
        head += 1;
        let last = *path.last().expect("a path always has a node");
        for (_, d) in EDGES.iter().filter(|(s, _)| *s == last) {
            if seen.contains(d) {
                continue;
            }
            let mut next = path.clone();
            next.push(d);
            if *d == to {
                return Some(next.windows(2).map(|w| (w[0], w[1])).collect());
            }
            seen.push(d);
            queue.push(next);
        }
    }
    None
}

/// The refusal `#primitive_errinfo` reports, and the `Symbol` naming it.
#[derive(Clone, Default)]
struct ErrInfo {
    /// `nil` here means the initial `:source_buffer_empty` state.
    result: Option<&'static str>,
    src: Option<EncodingId>,
    dst: Option<EncodingId>,
    error_bytes: Option<Vec<u8>>,
}

/// Everything a converter mutates as it runs.
struct ConvState {
    tstate: TranscodeState,
    opts: TranscodeOptions,
    /// Source bytes decoded no further -- a sequence cut short by the end of
    /// a chunk, waiting for the rest.
    pending: Vec<u8>,
    /// Output produced (or `insert_output`ed) but not yet handed back.
    carry: Vec<u8>,
    last_error: RubyValue,
    errinfo: ErrInfo,
    finished: bool,
}

pub struct RConverter {
    /// The REQUESTED encodings -- what `#source_encoding` answers, even when
    /// the path pivots through others. `None` for the no-conversion form.
    src: Option<EncodingId>,
    dst: Option<EncodingId>,
    flags: i64,
    state: Mutex<ConvState>,
}

impl RubyObject for RConverter {
    fn class_id(&self) -> ClassId {
        ENCODING_CONVERTER_CLASS
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn as_any_rc(self: Arc<Self>) -> Arc<dyn std::any::Any + Send + Sync> {
        self
    }
    fn is_frozen(&self) -> bool {
        false
    }
    fn set_frozen(&self) {}
    fn ivar_values(&self) -> Vec<RubyValue> {
        vec![self.state.lock().last_error.clone()]
    }
    fn dup_object(&self, _copy_frozen: bool) -> RObj {
        build(self.src, self.dst, self.flags).expect("the pair already built once")
    }
}

/// The target the engine writes through: the destination encoding, or raw
/// bytes for the no-conversion form (which only runs decorators).
fn engine_target(dst: Option<EncodingId>) -> EncodingId {
    dst.unwrap_or(encoding::ASCII_8BIT)
}

fn build(src: Option<EncodingId>, dst: Option<EncodingId>, flags: i64) -> Option<RObj> {
    let mut opts = TranscodeOptions {
        invalid_replace: flags & INVALID_REPLACE != 0,
        undef_replace: flags & UNDEF_REPLACE == UNDEF_REPLACE,
        ..Default::default()
    };
    if flags & UNDEF_HEX_CHARREF == UNDEF_HEX_CHARREF {
        opts.undef_replace = false;
        opts.xml = Some(XmlMode::Text);
    }
    if flags & XML_TEXT != 0 {
        opts.xml = Some(XmlMode::Text);
    }
    if flags & XML_ATTR_CONTENT != 0 {
        opts.xml = Some(XmlMode::Attr);
    }
    opts.newline = if flags & UNIVERSAL_NEWLINE != 0 {
        Some(NewlineMode::Universal)
    } else if flags & CRLF_NEWLINE != 0 {
        Some(NewlineMode::Crlf)
    } else if flags & CR_NEWLINE != 0 {
        Some(NewlineMode::Cr)
    } else {
        None
    };
    Some(Arc::new(RConverter {
        src,
        dst,
        flags,
        state: Mutex::new(ConvState {
            tstate: TranscodeState::new(engine_target(dst)),
            opts,
            pending: Vec::new(),
            carry: Vec::new(),
            last_error: RubyValue::Nil,
            errinfo: ErrInfo::default(),
            finished: false,
        }),
    }))
}

fn as_converter(recv: &RubyValue) -> Result<&RConverter, Signal> {
    let RubyValue::Object(o) = recv else {
        return Err(type_error!("not an Encoding::Converter"));
    };
    o.as_any()
        .downcast_ref::<RConverter>()
        .ok_or_else(|| type_error!("not an Encoding::Converter"))
}

/// The decorator flags set on `flags`, as `(convpath name, message name)`.
fn decorators(flags: i64) -> Vec<(&'static str, &'static str)> {
    DECORATORS
        .iter()
        .filter(|(bit, _, _)| flags & bit != 0)
        .map(|(_, path, msg)| (*path, *msg))
        .collect()
}

/// `code converter not found (...)`, with the decorator list CRuby appends
/// when there is one.
fn not_found(src: &str, dst: &str, flags: i64) -> Signal {
    let decs = decorators(flags);
    let tail = if decs.is_empty() {
        String::new()
    } else {
        let names: Vec<&str> = decs.iter().map(|(_, m)| *m).collect();
        format!(" with {}", names.join(","))
    };
    crate::dispatch::raise_error(
        "Encoding::ConverterNotFoundError",
        format!("code converter not found ({src} to {dst}{tail})"),
    )
}

/// Reads `Converter.new`'s third argument: an option Hash, an Integer mask,
/// or nothing. CRuby ignores anything else rather than refusing it.
fn read_flags(arg: Option<&RubyValue>) -> Result<i64, Signal> {
    let Some(v) = arg else { return Ok(0) };
    match v {
        RubyValue::Int(n) => Ok(*n),
        RubyValue::Hash(h) => {
            let get =
                |name: &str| crate::hash_get(h, &RubyValue::Symbol(crate::Symbol::intern(name)));
            let is =
                |v: &RubyValue, want: &str| matches!(v, RubyValue::Symbol(s) if s.name() == want);
            let mut flags = 0;
            if is(&get("invalid"), "replace") {
                flags |= INVALID_REPLACE;
            }
            match &get("undef") {
                v if is(v, "replace") => flags |= UNDEF_REPLACE,
                _ => {}
            }
            match &get("xml") {
                v if is(v, "text") => flags |= XML_TEXT,
                v if is(v, "attr") => flags |= XML_ATTR_CONTENT | XML_ATTR_QUOTE,
                _ => {}
            }
            for (name, bit) in [
                ("universal_newline", UNIVERSAL_NEWLINE),
                ("crlf_newline", CRLF_NEWLINE),
                ("cr_newline", CR_NEWLINE),
            ] {
                if get(name).truthy() {
                    flags |= bit;
                }
            }
            Ok(flags)
        }
        _ => Ok(0),
    }
}

/// One side of a converter, as the argument named it.
enum Side {
    /// An empty String: the no-conversion form.
    Blank,
    Enc(EncodingId),
    /// A name no encoding answers to. NOT an error by itself -- to CRuby it
    /// is a missing CONVERTER, which the pair reports together.
    Unknown,
}

/// `Converter.new`'s encoding argument: a String or an `Encoding`, never a
/// Symbol (CRuby refuses that with the `to_str` message).
fn arg_side(v: &RubyValue) -> Result<Side, Signal> {
    if let RubyValue::Object(o) = v {
        if o.class_id() == zeo_abi::ENCODING_CLASS {
            return Ok(Side::Enc(arg_encoding(v)?));
        }
    }
    // CRuby takes a String or an `Encoding` here and nothing else -- a
    // Symbol gets the plain `to_str` refusal rather than a name lookup.
    if matches!(v, RubyValue::Symbol(_)) {
        return Err(type_error!("no implicit conversion of Symbol into String"));
    }
    let name = convert::to_rstr(v)?.lock().to_utf8_lossy().into_owned();
    if name.is_empty() {
        return Ok(Side::Blank);
    }
    Ok(match encoding::find(&name) {
        Some(id) => Side::Enc(id),
        None => Side::Unknown,
    })
}

/// The name to report for a side that named no registered encoding.
fn side_name(v: &RubyValue) -> String {
    match v {
        RubyValue::Str(s) => s.lock().to_utf8_lossy().into_owned(),
        other => other.to_display_string(),
    }
}

fn converter_construct(
    _class: ClassId,
    args: &[RubyValue],
    _block: Option<RubyValue>,
) -> Result<RubyValue, Signal> {
    check_arity(args.len(), 2, Some(3))?;
    let flags = read_flags(args.get(2))?;
    let sides = [arg_side(&args[0])?, arg_side(&args[1])?];
    if sides.iter().any(|s| matches!(s, Side::Unknown)) {
        return Err(not_found(&side_name(&args[0]), &side_name(&args[1]), flags));
    }
    let side_enc = |s: &Side| match s {
        Side::Enc(id) => Some(*id),
        _ => None,
    };
    let (src, dst) = (side_enc(&sides[0]), side_enc(&sides[1]));
    // CRuby registers no converter from an encoding to itself, so a named
    // pair must differ. The no-conversion form (both sides empty) is the one
    // way to get a decorator-only converter.
    if let Some(same) = src.filter(|_| src == dst) {
        return Err(not_found(same.name(), same.name(), flags));
    }
    if let (Some(s), Some(d)) = (src, dst) {
        if convpath(s, d).is_none() {
            return Err(not_found(s.name(), d.name(), flags));
        }
    }
    // Two newline decorators cannot both apply.
    let newline_bits = [UNIVERSAL_NEWLINE, CRLF_NEWLINE, CR_NEWLINE]
        .iter()
        .filter(|b| flags & *b != 0)
        .count();
    if newline_bits > 1 {
        let (s, d) = (src.map_or("", |e| e.name()), dst.map_or("", |e| e.name()));
        return Err(not_found(s, d, flags));
    }
    Ok(RubyValue::Object(
        build(src, dst, flags).expect("validated above"),
    ))
}

/// Records a refusal on the converter and turns it into the Ruby exception,
/// which `#last_error` hands back afterwards.
fn refuse(
    state: &mut ConvState,
    result: &'static str,
    src: EncodingId,
    dst: EncodingId,
    error_bytes: Vec<u8>,
    err: encoding::TranscodeError,
) -> Signal {
    state.errinfo = ErrInfo {
        result: Some(result),
        src: Some(src),
        dst: Some(dst),
        error_bytes: Some(error_bytes),
    };
    let signal = encoding::transcode_signal(err);
    if let Signal::Raise(exc) = &signal {
        state.last_error = exc.clone();
    }
    signal
}

/// Converts everything buffered plus `input`, up to `limit` output bytes.
/// `final_chunk` says whether a cut-short tail is an error (`#finish`) or
/// simply more input to come.
fn run(
    conv: &RConverter,
    state: &mut ConvState,
    input: &[u8],
    limit: Option<usize>,
    final_chunk: bool,
) -> (Vec<u8>, Result<Stop, Signal>) {
    let src = conv.src.unwrap_or(encoding::ASCII_8BIT);
    let dst = engine_target(conv.dst);
    let mut buf = std::mem::take(&mut state.pending);
    buf.extend_from_slice(input);
    // `universal_newline` cannot answer a CR until it sees whether a LF
    // follows, so a CR ending a non-final chunk waits for the next one.
    let hold = usize::from(
        !final_chunk
            && state.opts.newline == Some(NewlineMode::Universal)
            && buf.last() == Some(&b'\r')
            && src.ascii_compatible(),
    );
    let held = buf.split_off(buf.len() - hold);
    let mut out = std::mem::take(&mut state.carry);
    let ConvState { tstate, opts, .. } = state;
    let mut r = encoding::transcode_run(&buf, src, opts, None, tstate, limit);
    let consumed = r.consumed;
    out.append(&mut r.out);
    let mut rest = buf.split_off(consumed);
    rest.extend_from_slice(&held);
    state.pending = rest;
    let stopped = match r.stop {
        Stop::Finished | Stop::DestinationFull => {
            state.errinfo = ErrInfo::default();
            Ok(r.stop)
        }
        Stop::Incomplete { bytes, msg, detail } => {
            if !final_chunk {
                // More input may still complete it, so it simply waits.
                state.errinfo = ErrInfo::default();
                Ok(Stop::Finished)
            } else if state.opts.invalid_replace {
                state.pending.clear();
                out.extend_from_slice(b"?");
                state.errinfo = ErrInfo::default();
                Ok(Stop::Finished)
            } else {
                Err(refuse(
                    state,
                    "incomplete_input",
                    src,
                    dst,
                    bytes,
                    encoding::TranscodeError::InvalidByteSequence(msg, detail),
                ))
            }
        }
        Stop::Invalid { msg, detail } => {
            let bytes = detail.error_bytes.clone();
            Err(refuse(
                state,
                "invalid_byte_sequence",
                src,
                dst,
                bytes,
                encoding::TranscodeError::InvalidByteSequence(msg, detail),
            ))
        }
        Stop::Undefined { msg, detail } => {
            let bytes = detail.error_bytes.clone();
            Err(refuse(
                state,
                "undefined_conversion",
                src,
                dst,
                bytes,
                encoding::TranscodeError::UndefinedConversion(msg, detail),
            ))
        }
        Stop::NoConverter(msg) => Err(refuse(
            state,
            "invalid_byte_sequence",
            src,
            dst,
            Vec::new(),
            encoding::TranscodeError::NoConverter(msg),
        )),
    };
    (out, stopped)
}

/// A converted chunk as a Ruby String, tagged with the destination encoding
/// (ASCII-8BIT for the no-conversion form, which CRuby also reports).
fn out_string(conv: &RConverter, bytes: Vec<u8>) -> RubyValue {
    RubyValue::Str(crate::string_from_bytes(bytes, engine_target(conv.dst)))
}

fn str_bytes(v: &RubyValue) -> Result<Vec<u8>, Signal> {
    Ok(convert::to_rstr(v)?.lock().bytes().to_vec())
}

pub fn register_converter(registry: &mut ClassRegistry) {
    registry.register(
        ENCODING_CONVERTER_CLASS,
        "Encoding::Converter",
        false,
        zeo_abi::declared_ancestors(ENCODING_CONVERTER_CLASS),
        Some(converter_construct as crate::dispatch::ConstructorFn),
    );
}

/// Seeds the option-mask constants -- called once from generated `main()`,
/// beside the encoding constants themselves.
pub fn seed_converter_constants() {
    for (name, value) in CONSTANTS {
        crate::const_set(ENCODING_CONVERTER_CLASS.0, name, RubyValue::Int(*value));
    }
}

ruby_class! {
    Converter = zeo_abi::ENCODING_CONVERTER_CLASS < zeo_abi::OBJECT_CLASS;

    // The ascii-compatible encoding a converter reads `arg` THROUGH, or nil.
    // An unknown name answers nil rather than raising, as CRuby does.
    def self."asciicompat_encoding"(_recv, arg) {
        let Ok(id) = arg_encoding(arg) else { return Ok(RubyValue::Nil) };
        Ok(match ASCII_COMPAT.iter().find(|(n, _)| *n == id.name()) {
            Some((_, target)) => encoding_value(encoding::find(target).expect("tabled name")),
            None => RubyValue::Nil,
        })
    }

    // The path a converter for this pair WOULD take, without building one.
    def self."search_convpath" cfunc (_recv, src, dst, opts?) {
        let flags = read_flags(opts)?;
        let sides = [arg_side(src)?, arg_side(dst)?];
        if sides.iter().any(|s| matches!(s, Side::Unknown)) {
            return Err(not_found(&side_name(src), &side_name(dst), flags));
        }
        let enc = |s: &Side| match s { Side::Enc(id) => Some(*id), _ => None };
        Ok(RubyValue::Array(crate::array_new(
            path_value(enc(&sides[0]), enc(&sides[1]), flags)?,
        )))
    }

    def "source_encoding"(recv) {
        Ok(match as_converter(recv)?.src {
            Some(id) => encoding_value(id),
            None => RubyValue::Nil,
        })
    }
    def "destination_encoding"(recv) {
        Ok(match as_converter(recv)?.dst {
            Some(id) => encoding_value(id),
            None => RubyValue::Nil,
        })
    }
    def "convpath"(recv) {
        let c = as_converter(recv)?;
        Ok(RubyValue::Array(crate::array_new(path_value(c.src, c.dst, c.flags)?)))
    }

    def "inspect"(recv) {
        let c = as_converter(recv)?;
        let decs: Vec<&str> = decorators(c.flags).iter().map(|(_, m)| *m).collect();
        let body = match (c.src, c.dst) {
            (Some(s), Some(d)) if decs.is_empty() => format!("{} to {}", s.name(), d.name()),
            (Some(s), Some(d)) => format!("{} to {} with {}", s.name(), d.name(), decs.join(",")),
            _ if decs.is_empty() => "no-conversion".to_string(),
            _ => decs.join(","),
        };
        Ok(RubyValue::Str(crate::string_new(format!("#<Encoding::Converter: {body}>"))))
    }

    // `nil`, not `false`, for a non-converter operand -- CRuby's own answer.
    def "=="(recv, other) {
        let c = as_converter(recv)?;
        let Ok(o) = as_converter(other) else { return Ok(RubyValue::Nil) };
        Ok(RubyValue::Bool(c.src == o.src && c.dst == o.dst && c.flags == o.flags))
    }

    def "convert"(recv, arg) {
        let c = as_converter(recv)?;
        let bytes = str_bytes(arg)?;
        let mut state = c.state.lock();
        if state.finished {
            return Err(arg_error!("converter already finished"));
        }
        let (out, stopped) = run(c, &mut state, &bytes, None, false);
        stopped?;
        Ok(out_string(c, out))
    }

    def "finish"(recv) {
        let c = as_converter(recv)?;
        let mut state = c.state.lock();
        let (mut out, stopped) = run(c, &mut state, &[], None, true);
        stopped?;
        let ConvState { tstate, .. } = &mut *state;
        tstate.finish(&mut out);
        state.finished = true;
        Ok(out_string(c, out))
    }

    // The low-level form: read from `src`, append to `dst`, and NAME how it
    // stopped rather than raising. Whatever is left unconverted stays in
    // `src` for the next call.
    def "primitive_convert" cfunc (recv, src, dst, dst_offset?, dst_bytesize?, flags?) {
        let c = as_converter(recv)?;
        let RubyValue::Str(dst_str) = dst else {
            return Err(type_error!(
                "no implicit conversion of {} into String",
                crate::builtins::convert_name_of(dst)
            ));
        };
        let input = match src {
            RubyValue::Nil => Vec::new(),
            other => str_bytes(other)?,
        };
        if let RubyValue::Str(s) = src {
            s.lock().replace_bytes(Vec::new(), c.src.unwrap_or(encoding::ASCII_8BIT));
        }
        if let Some(off) = dst_offset {
            if !matches!(off, RubyValue::Nil) {
                let at = crate::builtins::arg_int!(off) as usize;
                let kept: Vec<u8> = dst_str.lock().bytes().iter().take(at).copied().collect();
                let enc = dst_str.lock().encoding();
                dst_str.lock().replace_bytes(kept, enc);
            }
        }
        let limit = match dst_bytesize {
            Some(v) if !matches!(v, RubyValue::Nil) => Some(crate::builtins::arg_int!(v) as usize),
            _ => None,
        };
        let partial = flags.map_or(0, |f| if let RubyValue::Int(n) = f { *n } else { 0 })
            & 0x0002_0000 != 0;
        let mut state = c.state.lock();
        // Whatever converted before the stop is the caller's, refusal or not.
        let (out, stopped) = run(c, &mut state, &input, limit, !partial);
        append_bytes(dst_str, &out);
        // What the engine did not take goes back to the caller's source,
        // EXCEPT a cut-short tail, which this converter now holds.
        let give_back = match &stopped {
            Ok(Stop::Finished) => false,
            // A cut-short tail has moved INTO this converter, so the caller's
            // source is drained; every other refusal hands the rest back.
            Err(_) => state.errinfo.result != Some("incomplete_input"),
            _ => true,
        };
        if give_back {
            if let RubyValue::Str(s) = src {
                let rest = std::mem::take(&mut state.pending);
                s.lock()
                    .replace_bytes(rest, c.src.unwrap_or(encoding::ASCII_8BIT));
            }
        }
        // A refusal is NAMED here rather than raised; `run` has already
        // recorded it for `#last_error` and `#primitive_errinfo`.
        let name = match stopped {
            Err(Signal::Raise(_)) => state.errinfo.result.unwrap_or("invalid_byte_sequence"),
            Err(other) => return Err(other),
            Ok(Stop::DestinationFull) => "destination_buffer_full",
            Ok(_) if partial => "source_buffer_empty",
            Ok(_) if !state.pending.is_empty() => {
                state.errinfo = ErrInfo {
                    result: Some("incomplete_input"),
                    src: c.src,
                    dst: c.dst,
                    error_bytes: Some(state.pending.clone()),
                };
                "incomplete_input"
            }
            Ok(_) => "finished",
        };
        Ok(RubyValue::Symbol(crate::Symbol::intern(name)))
    }

    def "primitive_errinfo"(recv) {
        let c = as_converter(recv)?;
        let state = c.state.lock();
        let e = &state.errinfo;
        let name = e.result.unwrap_or("source_buffer_empty");
        let enc_name = |id: Option<EncodingId>| match id {
            Some(id) => RubyValue::Str(crate::string_new(id.name().to_string())),
            None => RubyValue::Nil,
        };
        let bytes = |b: &Option<Vec<u8>>| match b {
            Some(b) => RubyValue::Str(crate::string_from_bytes(b.clone(), encoding::ASCII_8BIT)),
            None => RubyValue::Nil,
        };
        Ok(RubyValue::Array(crate::array_new(vec![
            RubyValue::Symbol(crate::Symbol::intern(name)),
            enc_name(e.src),
            enc_name(e.dst),
            bytes(&e.error_bytes),
            // zeo's decoders consume an offending sequence whole, so there
            // are never bytes to read again -- see docs/COMPATIBILITY.md.
            if e.result.is_some() {
                RubyValue::Str(crate::string_from_bytes(Vec::new(), encoding::ASCII_8BIT))
            } else {
                RubyValue::Nil
            },
        ])))
    }

    def "last_error"(recv) {
        Ok(as_converter(recv)?.state.lock().last_error.clone())
    }

    // Splices text straight into the OUTPUT, ahead of whatever converts next.
    def "insert_output"(recv, arg) {
        let c = as_converter(recv)?;
        let text = convert::to_rstr(arg)?.lock().to_utf8_lossy().into_owned();
        let dst = engine_target(c.dst);
        let bytes = encoding::transcode(
            text.as_bytes(),
            encoding::UTF_8,
            dst,
            &TranscodeOptions::default(),
            None,
        )
        .map_err(encoding::transcode_signal)?;
        c.state.lock().carry.extend_from_slice(&bytes);
        Ok(RubyValue::Nil)
    }

    // The bytes to read again after an error. zeo's decoders never leave
    // any, so this is always empty -- documented, not silent.
    def "putback"(recv, _n?) {
        let c = as_converter(recv)?;
        Ok(RubyValue::Str(crate::string_from_bytes(
            Vec::new(),
            c.src.unwrap_or(encoding::ASCII_8BIT),
        )))
    }

    def "replacement"(recv) {
        let c = as_converter(recv)?;
        let state = c.state.lock();
        Ok(match &state.opts.replace {
            Some(r) => RubyValue::Str(crate::string_new(r.clone())),
            None => default_replacement(engine_target(c.dst)),
        })
    }
    def "replacement="(recv, arg) {
        let c = as_converter(recv)?;
        let text = convert::to_rstr(arg)?.lock().to_utf8_lossy().into_owned();
        // The replacement must be representable in the DESTINATION, which is
        // what CRuby checks here rather than at the first refusal.
        if encoding::transcode(
            text.as_bytes(),
            encoding::UTF_8,
            engine_target(c.dst),
            &TranscodeOptions::default(),
            None,
        )
        .is_err()
        {
            return Err(crate::dispatch::raise_error(
                "Encoding::UndefinedConversionError",
                "replacement character setup failed".to_string(),
            ));
        }
        c.state.lock().opts.replace = Some(text.clone());
        Ok(arg.clone())
    }
}

/// The `#replacement` a converter answers when none was set: U+FFFD for a
/// Unicode destination, `"?"` (US-ASCII) for every other.
fn default_replacement(dst: EncodingId) -> RubyValue {
    use crate::encoding::EncKind;
    match dst.kind() {
        EncKind::Utf8 | EncKind::Utf16 { .. } | EncKind::Utf32 { .. } => {
            RubyValue::Str(crate::string_new("\u{FFFD}".to_string()))
        }
        _ => RubyValue::Str(crate::string_from_bytes(b"?".to_vec(), encoding::US_ASCII)),
    }
}

/// `#convpath` / `.search_convpath`: the encoding hops, then the decorator
/// names -- xml before newline, which is the order CRuby applies them.
fn path_value(
    src: Option<EncodingId>,
    dst: Option<EncodingId>,
    flags: i64,
) -> Result<Vec<RubyValue>, Signal> {
    let mut out = Vec::new();
    if let (Some(s), Some(d)) = (src, dst) {
        let Some(hops) = convpath(s, d) else {
            return Err(not_found(s.name(), d.name(), flags));
        };
        for (a, b) in hops {
            let pair = vec![
                encoding_value(encoding::find(a).expect("tabled name")),
                encoding_value(encoding::find(b).expect("tabled name")),
            ];
            out.push(RubyValue::Array(crate::array_new(pair)));
        }
    }
    let decs = decorators(flags);
    for name in decs
        .iter()
        .filter(|(p, _)| p.starts_with("xml_"))
        .chain(decs.iter().filter(|(p, _)| !p.starts_with("xml_")))
    {
        out.push(RubyValue::Str(crate::string_new(name.0.to_string())));
    }
    Ok(out)
}

fn append_bytes(dst: &Arc<crate::collections::Freezable<crate::encoding::StrBuf>>, bytes: &[u8]) {
    let mut g = dst.lock();
    let mut all = g.bytes().to_vec();
    all.extend_from_slice(bytes);
    let enc = g.encoding();
    g.replace_bytes(all, enc);
}

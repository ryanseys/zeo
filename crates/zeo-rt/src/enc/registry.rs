//! The encoding registry: every row of ruby 4.0.6's `Encoding.list`, in its
//! order, so `Encoding.list`, `.name_list`, `.aliases`, `.find` and the
//! `Encoding::*` constants all answer exactly what CRuby answers.
//!
//! DERIVED from the oracle -- re-derive, do not edit. The oracle decides
//! the row ORDER, the names, the aliases, `dummy?` and `ascii_compatible?`;
//! a row is single-byte exactly when all 256 bytes each stand alone as one
//! valid character.
//!
//! A row's `kind` says how much of the encoding this runtime IMPLEMENTS.
//! [`EncKind::Registered`] rows carry a name, aliases and every reflection
//! answer, but no byte<->character mapping: they read as one byte per
//! character and refuse to transcode anything but ASCII.

use crate::enc::mb::MbFamily;
use crate::enc::single_byte;
use crate::enc::table::{EncKind, EncodingId, EncodingSpec};

pub const ASCII_8BIT: EncodingId = EncodingId(0);
pub const UTF_8: EncodingId = EncodingId(1);
pub const US_ASCII: EncodingId = EncodingId(2);
pub const UTF_16BE: EncodingId = EncodingId(3);
pub const UTF_16LE: EncodingId = EncodingId(4);
pub const UTF_32BE: EncodingId = EncodingId(5);
pub const UTF_32LE: EncodingId = EncodingId(6);
pub const UTF_16: EncodingId = EncodingId(7);
pub const UTF_32: EncodingId = EncodingId(8);
pub const UTF8_MAC: EncodingId = EncodingId(9);
pub const EUC_JP: EncodingId = EncodingId(10);
pub const WINDOWS_31J: EncodingId = EncodingId(11);
pub const BIG5: EncodingId = EncodingId(12);
pub const BIG5_HKSCS: EncodingId = EncodingId(13);
pub const BIG5_UAO: EncodingId = EncodingId(14);
pub const CESU_8: EncodingId = EncodingId(15);
pub const CP949: EncodingId = EncodingId(16);
pub const EMACS_MULE: EncodingId = EncodingId(17);
pub const EUC_KR: EncodingId = EncodingId(18);
pub const EUC_TW: EncodingId = EncodingId(19);
pub const GB18030: EncodingId = EncodingId(20);
pub const GBK: EncodingId = EncodingId(21);
pub const ISO_8859_1: EncodingId = EncodingId(22);
pub const ISO_8859_2: EncodingId = EncodingId(23);
pub const ISO_8859_3: EncodingId = EncodingId(24);
pub const ISO_8859_4: EncodingId = EncodingId(25);
pub const ISO_8859_5: EncodingId = EncodingId(26);
pub const ISO_8859_6: EncodingId = EncodingId(27);
pub const ISO_8859_7: EncodingId = EncodingId(28);
pub const ISO_8859_8: EncodingId = EncodingId(29);
pub const ISO_8859_9: EncodingId = EncodingId(30);
pub const ISO_8859_10: EncodingId = EncodingId(31);
pub const ISO_8859_11: EncodingId = EncodingId(32);
pub const ISO_8859_13: EncodingId = EncodingId(33);
pub const ISO_8859_14: EncodingId = EncodingId(34);
pub const ISO_8859_15: EncodingId = EncodingId(35);
pub const ISO_8859_16: EncodingId = EncodingId(36);
pub const KOI8_R: EncodingId = EncodingId(37);
pub const KOI8_U: EncodingId = EncodingId(38);
pub const SHIFT_JIS: EncodingId = EncodingId(39);
pub const WINDOWS_1250: EncodingId = EncodingId(40);
pub const WINDOWS_1251: EncodingId = EncodingId(41);
pub const WINDOWS_1252: EncodingId = EncodingId(42);
pub const WINDOWS_1253: EncodingId = EncodingId(43);
pub const WINDOWS_1254: EncodingId = EncodingId(44);
pub const WINDOWS_1257: EncodingId = EncodingId(45);
pub const IBM437: EncodingId = EncodingId(46);
pub const IBM720: EncodingId = EncodingId(47);
pub const IBM737: EncodingId = EncodingId(48);
pub const IBM775: EncodingId = EncodingId(49);
pub const CP850: EncodingId = EncodingId(50);
pub const IBM852: EncodingId = EncodingId(51);
pub const CP852: EncodingId = EncodingId(52);
pub const IBM855: EncodingId = EncodingId(53);
pub const CP855: EncodingId = EncodingId(54);
pub const IBM857: EncodingId = EncodingId(55);
pub const IBM860: EncodingId = EncodingId(56);
pub const IBM861: EncodingId = EncodingId(57);
pub const IBM862: EncodingId = EncodingId(58);
pub const IBM863: EncodingId = EncodingId(59);
pub const IBM864: EncodingId = EncodingId(60);
pub const IBM865: EncodingId = EncodingId(61);
pub const IBM866: EncodingId = EncodingId(62);
pub const IBM869: EncodingId = EncodingId(63);
pub const WINDOWS_1258: EncodingId = EncodingId(64);
pub const GB1988: EncodingId = EncodingId(65);
pub const MACCENTEURO: EncodingId = EncodingId(66);
pub const MACCROATIAN: EncodingId = EncodingId(67);
pub const MACCYRILLIC: EncodingId = EncodingId(68);
pub const MACGREEK: EncodingId = EncodingId(69);
pub const MACICELAND: EncodingId = EncodingId(70);
pub const MACROMAN: EncodingId = EncodingId(71);
pub const MACROMANIA: EncodingId = EncodingId(72);
pub const MACTHAI: EncodingId = EncodingId(73);
pub const MACTURKISH: EncodingId = EncodingId(74);
pub const MACUKRAINE: EncodingId = EncodingId(75);
pub const CP950: EncodingId = EncodingId(76);
pub const CP951: EncodingId = EncodingId(77);
pub const IBM037: EncodingId = EncodingId(78);
pub const STATELESS_ISO_2022_JP: EncodingId = EncodingId(79);
pub const EUCJP_MS: EncodingId = EncodingId(80);
pub const CP51932: EncodingId = EncodingId(81);
pub const EUC_JIS_2004: EncodingId = EncodingId(82);
pub const GB2312: EncodingId = EncodingId(83);
pub const GB12345: EncodingId = EncodingId(84);
pub const ISO_2022_JP: EncodingId = EncodingId(85);
pub const ISO_2022_JP_2: EncodingId = EncodingId(86);
pub const CP50220: EncodingId = EncodingId(87);
pub const CP50221: EncodingId = EncodingId(88);
pub const WINDOWS_1256: EncodingId = EncodingId(89);
pub const WINDOWS_1255: EncodingId = EncodingId(90);
pub const TIS_620: EncodingId = EncodingId(91);
pub const WINDOWS_874: EncodingId = EncodingId(92);
pub const MACJAPANESE: EncodingId = EncodingId(93);
pub const UTF_7: EncodingId = EncodingId(94);
pub const UTF8_DOCOMO: EncodingId = EncodingId(95);
pub const SJIS_DOCOMO: EncodingId = EncodingId(96);
pub const UTF8_KDDI: EncodingId = EncodingId(97);
pub const SJIS_KDDI: EncodingId = EncodingId(98);
pub const ISO_2022_JP_KDDI: EncodingId = EncodingId(99);
pub const STATELESS_ISO_2022_JP_KDDI: EncodingId = EncodingId(100);
pub const UTF8_SOFTBANK: EncodingId = EncodingId(101);
pub const SJIS_SOFTBANK: EncodingId = EncodingId(102);

pub static ENCODINGS: &[EncodingSpec] = &[
    EncodingSpec {
        name: "ASCII-8BIT",
        aliases: &["BINARY"],
        ascii_compatible: true,
        kind: EncKind::Binary,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "UTF-8",
        aliases: &["CP65001"],
        ascii_compatible: true,
        kind: EncKind::Utf8,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "US-ASCII",
        aliases: &["ASCII", "ANSI_X3.4-1968", "646"],
        ascii_compatible: true,
        kind: EncKind::Ascii,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "UTF-16BE",
        aliases: &["UCS-2BE"],
        ascii_compatible: false,
        kind: EncKind::Utf16 { be: true },
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "UTF-16LE",
        aliases: &[],
        ascii_compatible: false,
        kind: EncKind::Utf16 { be: false },
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "UTF-32BE",
        aliases: &["UCS-4BE"],
        ascii_compatible: false,
        kind: EncKind::Utf32 { be: true },
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "UTF-32LE",
        aliases: &["UCS-4LE"],
        ascii_compatible: false,
        kind: EncKind::Utf32 { be: false },
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "UTF-16",
        aliases: &[],
        ascii_compatible: false,
        kind: EncKind::Binary,
        table: None,
        dummy: true,
    },
    EncodingSpec {
        name: "UTF-32",
        aliases: &[],
        ascii_compatible: false,
        kind: EncKind::Binary,
        table: None,
        dummy: true,
    },
    EncodingSpec {
        name: "UTF8-MAC",
        aliases: &["UTF-8-MAC", "UTF-8-HFS"],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "EUC-JP",
        aliases: &["eucJP"],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::EucJp),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "Windows-31J",
        aliases: &["CP932", "csWindows31J", "SJIS", "PCK"],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::Sjis),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "Big5",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::Big5),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "Big5-HKSCS",
        aliases: &["Big5-HKSCS:2008"],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::Big5),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "Big5-UAO",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "CESU-8",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "CP949",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::Cp949),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "Emacs-Mule",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "EUC-KR",
        aliases: &["eucKR"],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::EucKr),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "EUC-TW",
        aliases: &["eucTW"],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "GB18030",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::Gb18030),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "GBK",
        aliases: &["CP936"],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::Gbk),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-1",
        aliases: &["ISO8859-1"],
        ascii_compatible: true,
        kind: EncKind::Latin1,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-2",
        aliases: &["ISO8859-2"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_2),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-3",
        aliases: &["ISO8859-3"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_3),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-4",
        aliases: &["ISO8859-4"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_4),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-5",
        aliases: &["ISO8859-5"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_5),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-6",
        aliases: &["ISO8859-6"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_6),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-7",
        aliases: &["ISO8859-7"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_7),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-8",
        aliases: &["ISO8859-8"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_8),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-9",
        aliases: &["ISO8859-9"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_9),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-10",
        aliases: &["ISO8859-10"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_10),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-11",
        aliases: &["ISO8859-11"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_11),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-13",
        aliases: &["ISO8859-13"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_13),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-14",
        aliases: &["ISO8859-14"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_14),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-15",
        aliases: &["ISO8859-15"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_15),
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-8859-16",
        aliases: &["ISO8859-16"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::ISO_8859_16),
        dummy: false,
    },
    EncodingSpec {
        name: "KOI8-R",
        aliases: &["CP878"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::KOI8_R),
        dummy: false,
    },
    EncodingSpec {
        name: "KOI8-U",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::KOI8_U),
        dummy: false,
    },
    EncodingSpec {
        name: "Shift_JIS",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::Sjis),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "Windows-1250",
        aliases: &["CP1250"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::WINDOWS_1250),
        dummy: false,
    },
    EncodingSpec {
        name: "Windows-1251",
        aliases: &["CP1251"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::WINDOWS_1251),
        dummy: false,
    },
    EncodingSpec {
        name: "Windows-1252",
        aliases: &["CP1252"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::WINDOWS_1252),
        dummy: false,
    },
    EncodingSpec {
        name: "Windows-1253",
        aliases: &["CP1253"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::WINDOWS_1253),
        dummy: false,
    },
    EncodingSpec {
        name: "Windows-1254",
        aliases: &["CP1254"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::WINDOWS_1254),
        dummy: false,
    },
    EncodingSpec {
        name: "Windows-1257",
        aliases: &["CP1257"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::WINDOWS_1257),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM437",
        aliases: &["CP437"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM437),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM720",
        aliases: &["CP720"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM720),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM737",
        aliases: &["CP737"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM737),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM775",
        aliases: &["CP775"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM775),
        dummy: false,
    },
    EncodingSpec {
        name: "CP850",
        aliases: &["IBM850"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::CP850),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM852",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM852),
        dummy: false,
    },
    EncodingSpec {
        name: "CP852",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::CP852),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM855",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM855),
        dummy: false,
    },
    EncodingSpec {
        name: "CP855",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::CP855),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM857",
        aliases: &["CP857"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM857),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM860",
        aliases: &["CP860"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM860),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM861",
        aliases: &["CP861"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM861),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM862",
        aliases: &["CP862"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM862),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM863",
        aliases: &["CP863"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM863),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM864",
        aliases: &["CP864"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM864),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM865",
        aliases: &["CP865"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM865),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM866",
        aliases: &["CP866"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM866),
        dummy: false,
    },
    EncodingSpec {
        name: "IBM869",
        aliases: &["CP869"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::IBM869),
        dummy: false,
    },
    EncodingSpec {
        name: "Windows-1258",
        aliases: &["CP1258"],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "GB1988",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "macCentEuro",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "macCroatian",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::MACCROATIAN),
        dummy: false,
    },
    EncodingSpec {
        name: "macCyrillic",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::MACCYRILLIC),
        dummy: false,
    },
    EncodingSpec {
        name: "macGreek",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::MACGREEK),
        dummy: false,
    },
    EncodingSpec {
        name: "macIceland",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::MACICELAND),
        dummy: false,
    },
    EncodingSpec {
        name: "macRoman",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::MACROMAN),
        dummy: false,
    },
    EncodingSpec {
        name: "macRomania",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::MACROMANIA),
        dummy: false,
    },
    EncodingSpec {
        name: "macThai",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "macTurkish",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::MACTURKISH),
        dummy: false,
    },
    EncodingSpec {
        name: "macUkraine",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::MACUKRAINE),
        dummy: false,
    },
    EncodingSpec {
        name: "CP950",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "CP951",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "IBM037",
        aliases: &["ebcdic-cp-us"],
        ascii_compatible: false,
        kind: EncKind::Registered,
        table: None,
        dummy: true,
    },
    EncodingSpec {
        name: "stateless-ISO-2022-JP",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "eucJP-ms",
        aliases: &["euc-jp-ms"],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::EucJp),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "CP51932",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::EucJp),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "EUC-JIS-2004",
        aliases: &["EUC-JISX0213"],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "GB2312",
        aliases: &["EUC-CN", "eucCN"],
        ascii_compatible: true,
        kind: EncKind::MultiByte(MbFamily::Gbk),
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "GB12345",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-2022-JP",
        aliases: &["ISO2022-JP"],
        ascii_compatible: false,
        kind: EncKind::Binary,
        table: None,
        dummy: true,
    },
    EncodingSpec {
        name: "ISO-2022-JP-2",
        aliases: &["ISO2022-JP2"],
        ascii_compatible: false,
        kind: EncKind::Registered,
        table: None,
        dummy: true,
    },
    EncodingSpec {
        name: "CP50220",
        aliases: &[],
        ascii_compatible: false,
        kind: EncKind::Registered,
        table: None,
        dummy: true,
    },
    EncodingSpec {
        name: "CP50221",
        aliases: &[],
        ascii_compatible: false,
        kind: EncKind::Registered,
        table: None,
        dummy: true,
    },
    EncodingSpec {
        name: "Windows-1256",
        aliases: &["CP1256"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::WINDOWS_1256),
        dummy: false,
    },
    EncodingSpec {
        name: "Windows-1255",
        aliases: &["CP1255"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::WINDOWS_1255),
        dummy: false,
    },
    EncodingSpec {
        name: "TIS-620",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::TIS_620),
        dummy: false,
    },
    EncodingSpec {
        name: "Windows-874",
        aliases: &["CP874"],
        ascii_compatible: true,
        kind: EncKind::SingleByte,
        table: Some(&single_byte::WINDOWS_874),
        dummy: false,
    },
    EncodingSpec {
        name: "MacJapanese",
        aliases: &["MacJapan"],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "UTF-7",
        aliases: &["CP65000"],
        ascii_compatible: false,
        kind: EncKind::Registered,
        table: None,
        dummy: true,
    },
    EncodingSpec {
        name: "UTF8-DoCoMo",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "SJIS-DoCoMo",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "UTF8-KDDI",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "SJIS-KDDI",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "ISO-2022-JP-KDDI",
        aliases: &[],
        ascii_compatible: false,
        kind: EncKind::Registered,
        table: None,
        dummy: true,
    },
    EncodingSpec {
        name: "stateless-ISO-2022-JP-KDDI",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "UTF8-SoftBank",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
    EncodingSpec {
        name: "SJIS-SoftBank",
        aliases: &[],
        ascii_compatible: true,
        kind: EncKind::Registered,
        table: None,
        dummy: false,
    },
];

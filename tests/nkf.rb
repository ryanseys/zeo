# nkf -- Network Kanji Filter over zeo's own encoding engine: conversion
# between UTF-8/Shift_JIS/EUC-JP/ISO-2022-JP (and the UTF-16/32 forms),
# encoding detection, and the text passes (MIME words, katakana folding,
# -Z, newlines). ISO-2022-JP strings print via unpack1("H*") -- bytes, not
# rendering, are the contract.
require "nkf"

p defined?(NKF)
p NKF::VERSION
p NKF::NKF_VERSION
p [NKF::AUTO, NKF::NOCONV, NKF::UNKNOWN]
p [NKF::JIS, NKF::EUC, NKF::SJIS, NKF::UTF8, NKF::UTF16, NKF::UTF32, NKF::ASCII, NKF::BINARY]

utf = "こんにちは世界"
sj = utf.encode("Shift_JIS")
euc = utf.encode("EUC-JP")
jis = utf.encode("ISO-2022-JP")

p NKF.guess("hello")
p NKF.guess(utf)
p NKF.guess(sj.b)
p NKF.guess(euc.b)
p NKF.guess(jis.b)
p NKF.guess("\xFF\xFE\x41\x00".b)
p NKF.guess("\x00\x00\xFE\xFF".b)

r = NKF.nkf("-w", sj.b); p [r, r.encoding]
r = NKF.nkf("-w", euc.b); p [r, r.encoding]
r = NKF.nkf("-w", jis.b); p [r, r.encoding]
r = NKF.nkf("-s", utf); p [r == sj, r.encoding]
r = NKF.nkf("-e", utf); p [r == euc, r.encoding]
r = NKF.nkf("-j", utf); p [r.unpack1("H*"), r.encoding]
p NKF.nkf("-S -w", sj.b)
p NKF.nkf("--ic=Shift_JIS --oc=UTF-8", sj.b)
r = NKF.nkf("-w16", "あ"); p [r.encoding, r.unpack1("H*")]
r = NKF.nkf("-w16L", "あ"); p [r.encoding, r.unpack1("H*")]
r = NKF.nkf("--oc=UTF-16", "あ"); p [r.encoding, r.unpack1("H*")]
r = NKF.nkf("-w", "hello"); p [r, r.encoding]

# Halfwidth katakana fold to fullwidth by default (voiced marks combine);
# -x keeps them; -j -x emits a real ESC ( I run.
p NKF.nkf("-w", "ﾃｽﾄ ｶﾞｷﾞﾊﾟ")
p NKF.nkf("-w -x", "ﾃｽﾄ")
p NKF.nkf("-j -x", "ｱｲ").unpack1("H*")

# MIME encoded words decode by default; -m0 turns it off.
p NKF.nkf("-w", "=?UTF-8?B?44OG44K544OI?=")
p NKF.nkf("-w -m0", "=?UTF-8?B?44OG44K544OI?=")
p NKF.nkf("-w", "=?ISO-2022-JP?B?GyRCJUYlOSVIGyhC?=")
p NKF.nkf("-wm", "a =?utf-8?q?te=20st_x?= b")

p NKF.nkf("-w -Z", "Ａｂｃ　１２３！？")
p NKF.nkf("-w -Z1", "Ａ　ｂ")
p NKF.nkf("-w -Z2", "Ａ　ｂ")

p NKF.nkf("-w -Lu", "a\r\nb\rc\n")
p NKF.nkf("-w -Lw", "a\nb").unpack1("H*")
p NKF.nkf("-w -Lm", "a\nb").unpack1("H*")

begin; NKF.nkf("-x", "abc"); rescue => e; p [e.class, e.message]; end
begin; NKF.nkf(:w, "x"); rescue => e; p [e.class, e.message]; end
begin; NKF.nkf("-w"); rescue => e; p [e.class, e.message]; end

# The ISO-2022-JP dummy encoding behind NKF::JIS, via String#encode.
s = utf.encode("ISO-2022-JP")
p [s.encoding.dummy?, s.encoding.ascii_compatible?, s.length == s.bytesize, s.valid_encoding?]
p s.encode("UTF-8") == utf
p Encoding::ISO_2022_JP.names

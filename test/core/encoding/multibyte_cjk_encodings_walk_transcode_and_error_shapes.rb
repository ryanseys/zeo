# The multibyte CJK rows (Shift_JIS / Windows-31J / EUC-JP / GBK / Big5):
# structural walking is CRuby-faithful (a valid-but-unmapped pair is a real
# character), mapping via encoding_rs; char ops, transcode round trips,
# Integer#chr / << codepoint splits, and all three
# InvalidByteSequenceError message forms. Verbatim ruby 4.0.6 output.

sj = "あいz".encode("Shift_JIS")
p sj.bytes
p sj.length
p sj.valid_encoding?
p sj
p sj.reverse.bytes
p sj[0]&.bytes
p sj.encode("UTF-8")
e = "日本語".encode("EUC-JP")
p e.bytes
p e.length
p e.encode("UTF-8")
g = "中文".encode("GBK")
p g.bytes
b5 = "中文".encode("Big5")
p b5.bytes
p g.encode("Big5") == b5
w = "アｱ".encode("Windows-31J")
p w.bytes
p w.encoding
p 0x82A0.chr(Encoding::Shift_JIS).bytes
s2 = "".force_encoding("Shift_JIS"); s2 << 0x82A0; s2 << 65; p s2.bytes
begin; 0x8200.chr(Encoding::Shift_JIS); rescue RangeError => ex; puts ex.message; end
bad = 0x82.chr.force_encoding("Shift_JIS")
p bad.valid_encoding?
p bad.length
p bad
begin; bad.encode("UTF-8"); rescue Encoding::InvalidByteSequenceError => ex; puts ex.message; end
begin; (0x82.chr + 0x00.chr).force_encoding("Shift_JIS").encode("UTF-8"); rescue Encoding::InvalidByteSequenceError => ex; puts ex.message; end
begin; (0x82.chr + "z").force_encoding("Shift_JIS").encode("UTF-8"); rescue Encoding::UndefinedConversionError => ex; puts ex.message; end
kana = 0xB1.chr.force_encoding("Shift_JIS")
p kana.length
p kana.encode("UTF-8")
__END__
[130, 160, 130, 162, 122]
3
true
"\x{82A0}\x{82A2}z"
[122, 130, 162, 130, 160]
[130, 160]
"あいz"
[198, 252, 203, 220, 184, 236]
3
"日本語"
[214, 208, 206, 196]
[164, 164, 164, 229]
true
[131, 65, 177]
#<Encoding:Windows-31J>
[130, 160]
[130, 160, 65]
invalid codepoint 0x8200 in Shift_JIS
false
1
"\x82"
incomplete "\x82" on Shift_JIS
"\x82" followed by "\x00" on Shift_JIS
"\x82z" from Shift_JIS to UTF-8
1
"ｱ"

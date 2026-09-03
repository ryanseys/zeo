pl = "żółć gęś"
w = pl.encode("Windows-1250")
p w.bytes
p w.encoding.name
p w.encode("UTF-8") == pl
r = "привет".encode("Windows-1251")
p r.bytes
p r.encode("KOI8-R").bytes
p "€uro".encode("ISO-8859-15").bytes
b = 0x81.chr.force_encoding("Windows-1252")
begin; b.encode("UTF-8"); rescue Encoding::UndefinedConversionError => e; puts e.message; end
begin; b.encode("KOI8-R"); rescue Encoding::UndefinedConversionError => e; puts e.message; end
p b.encode("UTF-8", undef: :replace)
begin; "щ".encode("Windows-1252"); rescue Encoding::UndefinedConversionError => e; puts e.message; end
s = (0xE9.chr + 0x81.chr + "ab").force_encoding("Windows-1252")
p s
p (0xE9.chr + "ab").force_encoding("Windows-1252").encode("UTF-8")
__END__
[191, 243, 179, 230, 32, 103, 234, 156]
"Windows-1250"
true
[239, 240, 232, 226, 229, 242]
[208, 210, 201, 215, 197, 212]
[164, 117, 114, 111]
"\x81" to UTF-8 in conversion from Windows-1252 to UTF-8
"\x81" to UTF-8 in conversion from Windows-1252 to UTF-8 to KOI8-R
"�"
U+0449 to WINDOWS-1252 in conversion from UTF-8 to WINDOWS-1252
"\xE9\x81ab"
"éab"

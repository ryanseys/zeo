s = ("caf" + 0xE9.chr).force_encoding("Windows-1252")
p s.valid_encoding?
p s.length
p s.upcase.bytes
puts ("stra" + 0xDF.chr + "e").force_encoding("Windows-1252").upcase
p 0x81.chr.force_encoding("Windows-1252").valid_encoding?
p Encoding.find("cp1250")
p Encoding::Windows_1251.name
p Encoding::CP1252 == Encoding::Windows_1252
koi = (0xC1.chr + "z").force_encoding("KOI8-R")
p koi.upcase.bytes
p 0xB1.chr.force_encoding("ISO-8859-2").upcase.bytes
p 0xFF.chr.force_encoding("Windows-1252").upcase.bytes
__END__
true
4
[67, 65, 70, 201]
STRASSE
true
#<Encoding:Windows-1250>
"Windows-1251"
true
[193, 90]
[161]
[159]

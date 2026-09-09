# Ruby REFUSES a String operation that has to READ characters when the
# receiver's bytes are not valid in its own encoding. zeo renders a bad byte as
# U+FFFD (`StrBuf::to_utf8_lossy`) and used to answer anyway.
#
# The gate is selective, exactly as CRuby's is: `#length`, `#chars`, `#reverse`,
# `#lstrip`, `#index`, `#slice` and friends count or move by bytes and answer
# fine on a broken string. Only a row that interprets a character raises -- and
# it raises in one of four shapes, because CRuby reaches the check from four
# different places. All oracle-verified.

s = "caf\xC3 x".dup.force_encoding("UTF-8")
p s.valid_encoding?

def show(label)
  r = yield
  puts "#{label}\tOK #{r.inspect}"
rescue Exception => e
  puts "#{label}\t#{e.class}\t#{e.message}"
end

# ArgumentError "input string invalid" -- the case-mapping family, which
# raises out of the case-fold pass rather than the encoding check.
show("upcase")     { s.upcase }
show("downcase")   { s.downcase }
show("capitalize") { s.capitalize }
show("swapcase")   { s.swapcase }
show("casecmp?")   { s.casecmp?("x") }

# ArgumentError "invalid byte sequence in UTF-8" -- everything that walks
# characters to do its work.
show("squeeze")   { s.squeeze }
show("codepoints"){ s.codepoints }
show("count")     { s.count("x") }
show("delete")    { s.delete("x") }
show("tr")        { s.tr("x", "y") }
show("scan")      { s.scan(/x/) }
show("split")     { s.split(" ") }
show("sub")       { s.sub(/x/, "y") }
show("gsub")      { s.gsub(/x/, "y") }
show("match")     { s.match(/x/) }
show("normalize") { s.unicode_normalize }

# Encoding::CompatibilityError -- `strip`/`rstrip` scan BACKWARD and reach the
# check through `rb_enc_check`. `lstrip` scans forward and does not raise.
show("strip")  { s.strip }
show("rstrip") { s.rstrip }
show("lstrip") { s.lstrip }

# EncodingError -- a symbol carries its bytes, so ruby refuses to mint one it
# could not spell back, and echoes the would-be symbol.
show("to_sym") { s.to_sym }
show("intern") { s.intern }

# The rows that deliberately do NOT gate.
show("length")  { s.length }
show("size")    { s.size }
show("chars")   { s.chars }
show("bytes")   { s.bytes.size }
show("reverse") { s.reverse.bytes.size }
show("chomp")   { s.chomp.bytes.size }
show("index")   { s.index("x") }
show("include") { s.include?("x") }
show("slice")   { s.slice(0, 2) }
show("ord")     { s.ord }
show("succ")    { s.succ.bytes.size }
show("b")       { s.b.encoding }
show("valid")   { s.valid_encoding? }
show("ascii")   { s.ascii_only? }

# `#encode` is the converter's own error, and it names the byte that broke the
# sequence: a valid multi-byte PREFIX reports what followed it, a byte that is
# no lead at all is plain, and a prefix cut off by end-of-string is incomplete.
{
  "C3_space"  => "a\xC3 b",
  "C3_end"    => "a\xC3",
  "FF_space"  => "a\xFF b",
  "E3_81_sp"  => "a\xE3\x81 b",
  "E3_81_end" => "a\xE3\x81",
  "C2_41"     => "a\xC2Ab",
  "C0_space"  => "a\xC0 b",
  "F5_space"  => "a\xF5 b",
  "80_alone"  => "a\x80b",
  "E0_80"     => "a\xE0\x80b",
  "ED_A0"     => "a\xED\xA0\x80b",
  "F4_90"     => "a\xF4\x90\x80\x80b",
}.each do |label, bytes|
  show(label) { bytes.dup.force_encoding("UTF-8").encode("UTF-16").bytes.size }
end

# A VALID string is untouched by any of it.
ok = "café x"
p [ok.upcase, ok.strip, ok.to_sym, ok.count("é"), ok.split(" ")]
__END__
false
upcase	ArgumentError	input string invalid
downcase	ArgumentError	input string invalid
capitalize	ArgumentError	input string invalid
swapcase	ArgumentError	input string invalid
casecmp?	ArgumentError	input string invalid
squeeze	ArgumentError	invalid byte sequence in UTF-8
codepoints	ArgumentError	invalid byte sequence in UTF-8
count	ArgumentError	invalid byte sequence in UTF-8
delete	ArgumentError	invalid byte sequence in UTF-8
tr	ArgumentError	invalid byte sequence in UTF-8
scan	ArgumentError	invalid byte sequence in UTF-8
split	ArgumentError	invalid byte sequence in UTF-8
sub	ArgumentError	invalid byte sequence in UTF-8
gsub	ArgumentError	invalid byte sequence in UTF-8
match	ArgumentError	invalid byte sequence in UTF-8
normalize	ArgumentError	invalid byte sequence in UTF-8
strip	Encoding::CompatibilityError	invalid byte sequence in UTF-8
rstrip	Encoding::CompatibilityError	invalid byte sequence in UTF-8
lstrip	OK "caf\xC3 x"
to_sym	EncodingError	invalid symbol in encoding UTF-8 :"caf\xC3 x"
intern	EncodingError	invalid symbol in encoding UTF-8 :"caf\xC3 x"
length	OK 6
size	OK 6
chars	OK ["c", "a", "f", "\xC3", " ", "x"]
bytes	OK 6
reverse	OK 6
chomp	OK 6
index	OK 5
include	OK true
slice	OK "ca"
ord	OK 99
succ	OK 6
b	OK #<Encoding:BINARY (ASCII-8BIT)>
valid	OK false
ascii	OK false
C3_space	Encoding::InvalidByteSequenceError	"\xC3" followed by " " on UTF-8
C3_end	Encoding::InvalidByteSequenceError	incomplete "\xC3" on UTF-8
FF_space	Encoding::InvalidByteSequenceError	"\xFF" on UTF-8
E3_81_sp	Encoding::InvalidByteSequenceError	"\xE3\x81" followed by " " on UTF-8
E3_81_end	Encoding::InvalidByteSequenceError	incomplete "\xE3\x81" on UTF-8
C2_41	Encoding::InvalidByteSequenceError	"\xC2" followed by "A" on UTF-8
C0_space	Encoding::InvalidByteSequenceError	"\xC0" on UTF-8
F5_space	Encoding::InvalidByteSequenceError	"\xF5" on UTF-8
80_alone	Encoding::InvalidByteSequenceError	"\x80" on UTF-8
E0_80	Encoding::InvalidByteSequenceError	"\xE0" followed by "\x80" on UTF-8
ED_A0	Encoding::InvalidByteSequenceError	"\xED" followed by "\xA0" on UTF-8
F4_90	Encoding::InvalidByteSequenceError	"\xF4" followed by "\x90" on UTF-8
["CAFÉ X", "café x", :"café x", 1, ["café", "x"]]

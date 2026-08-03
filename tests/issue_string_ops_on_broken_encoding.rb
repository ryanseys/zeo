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
rescue Exception => e # rubocop:disable Lint/RescueException
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

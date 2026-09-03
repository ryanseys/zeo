# A Symbol remembers the encoding of the String it came from, so
# `str.to_sym.to_s` answers that encoding and those bytes.
#
# This was a gap. The interner used to key on `&'static str` and store nothing
# else, so `Symbol::intern` was handed `to_utf8_lossy` text and the encoding
# was gone by the time the id existed. Ruby interns by BYTES plus encoding --
# `:"caf\xE9"` in ISO-8859-1 and the same bytes in UTF-8 are two different
# symbols -- and so does zeo now.
#
# Each row pins the round trip for one non-UTF-8 encoding.
{
  "latin1" => "caf\xE9 x".dup.force_encoding("ISO-8859-1"),
  "binary" => "caf\xE9 x".dup.force_encoding("ASCII-8BIT"),
  "koi8"   => "\xC1\xC2 x".dup.force_encoding("KOI8-R"),
  "eucjp"  => "\xA4\xA2\xA4\xA4 x".dup.force_encoding("EUC-JP"),
}.each do |k, s|
  back = s.to_sym.to_s
  puts "#{k}\t#{back.encoding} #{back.bytes.inspect}"
end
__END__
latin1	ISO-8859-1 [99, 97, 102, 233, 32, 120]
binary	ASCII-8BIT [99, 97, 102, 233, 32, 120]
koi8	KOI8-R [193, 194, 32, 120]
eucjp	EUC-JP [164, 162, 164, 164, 32, 120]

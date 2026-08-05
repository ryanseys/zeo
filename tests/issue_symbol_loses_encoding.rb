# A Symbol does not remember the encoding of the String it came from, so
# `str.to_sym.to_s` answers UTF-8 whatever `str` was.
#
# The interner (crates/zeo-rt/src/symbol.rs) keys on `&'static str` and stores
# nothing else, so `Symbol::intern` is handed `to_utf8_lossy` text and the
# encoding is gone by the time the id exists. Ruby interns by BYTES plus
# encoding -- `:"caf\xE9"` in ISO-8859-1 and the same bytes in UTF-8 are two
# different symbols -- so this needs the interner to carry an encoding per
# name, not just a re-encode at the boundary.
{
  "latin1" => "caf\xE9 x".dup.force_encoding("ISO-8859-1"),
  "binary" => "caf\xE9 x".dup.force_encoding("ASCII-8BIT"),
  "koi8"   => "\xC1\xC2 x".dup.force_encoding("KOI8-R"),
  "eucjp"  => "\xA4\xA2\xA4\xA4 x".dup.force_encoding("EUC-JP"),
}.each do |k, s|
  back = s.to_sym.to_s
  puts "#{k}\t#{back.encoding} #{back.bytes.inspect}"
end

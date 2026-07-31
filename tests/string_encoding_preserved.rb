# A derived string keeps the RECEIVER's encoding and its raw bytes.
#
# The strip/pad/chop/tr/delete/squeeze family used to compute over
# `to_utf8_lossy` text and rebuild with `str_value`, which tags UTF-8 -- so
# `"caf\xE9".force_encoding("ISO-8859-1").strip` came back UTF-8 with `é` as
# two bytes where ruby keeps ISO-8859-1 and one. `chars`/`each_char` slice the
# raw bytes instead of decoding, and `codepoints` answers the number the
# receiver's own encoding gives a character, not the Unicode scalar.

def show(label)
  v = yield
  s = v.is_a?(String) ? "#{v.encoding} #{v.bytes.inspect}" : v.inspect
  puts "#{label}\t#{s}"
end

{
  "latin1" => "caf\xE9 x".dup.force_encoding("ISO-8859-1"),
  "binary" => "caf\xE9 x".dup.force_encoding("ASCII-8BIT"),
  "koi8"   => "\xC1\xC2 x".dup.force_encoding("KOI8-R"),
  "eucjp"  => "\xA4\xA2\xA4\xA4 x".dup.force_encoding("EUC-JP"),
}.each do |k, s|
  show("#{k}/strip")      { s.strip }
  show("#{k}/lstrip")     { s.lstrip }
  show("#{k}/rstrip")     { s.rstrip }
  show("#{k}/chop")       { s.chop }
  show("#{k}/center9")    { s.center(9, ".") }
  show("#{k}/ljust9")     { s.ljust(9, ".") }
  show("#{k}/rjust9")     { s.rjust(9, ".") }
  show("#{k}/delete_x")   { s.delete("x") }
  show("#{k}/squeeze")    { s.squeeze }
  show("#{k}/tr")         { s.tr("x", "z") }
  show("#{k}/chars")      { s.chars.map { |c| "#{c.encoding} #{c.bytes.inspect}" }.inspect }
  show("#{k}/codepoints") { s.codepoints.inspect }
end

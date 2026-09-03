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
  # Printed directly, not through `show`: `show` reports its argument's own
  # encoding, and `Array#inspect` answers US-ASCII in ruby where zeo answers
  # UTF-8 -- a separate divergence this test is not about.
  puts "#{k}/chars\t#{s.chars.map { |c| "#{c.encoding} #{c.bytes.inspect}" }}"
  puts "#{k}/codepoints\t#{s.codepoints}"
end

# The regexp-backed family: every field and every match is a slice of the
# receiver's own text, so it carries the receiver's encoding even though the
# splitter and the regexp engine both work in decoded UTF-8.
{
  "latin1" => "caf\xE9 x".dup.force_encoding("ISO-8859-1"),
  "binary" => "caf\xE9 x".dup.force_encoding("ASCII-8BIT"),
  "koi8"   => "\xC1\xC2 x".dup.force_encoding("KOI8-R"),
  "eucjp"  => "\xA4\xA2\xA4\xA4 x".dup.force_encoding("EUC-JP"),
}.each do |k, s|
  show("#{k}/sub")   { s.sub("x", "y") }
  show("#{k}/gsub")  { s.gsub("x", "y") }
  show("#{k}/sub_re") { s.sub(/x/, "y") }
  puts "#{k}/split_sp\t#{s.split(" ").map { |f| "#{f.encoding} #{f.bytes.inspect}" }}"
  puts "#{k}/split_empty\t#{s.split("").map { |f| "#{f.encoding} #{f.bytes.inspect}" }}"
  puts "#{k}/scan\t#{s.scan(/x/).map { |f| "#{f.encoding} #{f.bytes.inspect}" }}"
end

# Match groups are slices of the receiver's own text, so they carry the
# receiver's encoding: the engine decodes to UTF-8 and MatchData remembers
# what it decoded FROM.
{
  "latin1" => "caf\xE9 x".dup.force_encoding("ISO-8859-1"),
  "koi8"   => "\xC1\xC2 x".dup.force_encoding("KOI8-R"),
  "eucjp"  => "\xA4\xA2\xA4\xA4 x".dup.force_encoding("EUC-JP"),
}.each do |k, s|
  show("#{k}/match0")   { s.match(/x/)[0] }
  show("#{k}/match_pre") { s.match(/x/).pre_match }
end
__END__
latin1/strip	ISO-8859-1 [99, 97, 102, 233, 32, 120]
latin1/lstrip	ISO-8859-1 [99, 97, 102, 233, 32, 120]
latin1/rstrip	ISO-8859-1 [99, 97, 102, 233, 32, 120]
latin1/chop	ISO-8859-1 [99, 97, 102, 233, 32]
latin1/center9	ISO-8859-1 [46, 99, 97, 102, 233, 32, 120, 46, 46]
latin1/ljust9	ISO-8859-1 [99, 97, 102, 233, 32, 120, 46, 46, 46]
latin1/rjust9	ISO-8859-1 [46, 46, 46, 99, 97, 102, 233, 32, 120]
latin1/delete_x	ISO-8859-1 [99, 97, 102, 233, 32]
latin1/squeeze	ISO-8859-1 [99, 97, 102, 233, 32, 120]
latin1/tr	ISO-8859-1 [99, 97, 102, 233, 32, 122]
latin1/chars	["ISO-8859-1 [99]", "ISO-8859-1 [97]", "ISO-8859-1 [102]", "ISO-8859-1 [233]", "ISO-8859-1 [32]", "ISO-8859-1 [120]"]
latin1/codepoints	[99, 97, 102, 233, 32, 120]
binary/strip	ASCII-8BIT [99, 97, 102, 233, 32, 120]
binary/lstrip	ASCII-8BIT [99, 97, 102, 233, 32, 120]
binary/rstrip	ASCII-8BIT [99, 97, 102, 233, 32, 120]
binary/chop	ASCII-8BIT [99, 97, 102, 233, 32]
binary/center9	ASCII-8BIT [46, 99, 97, 102, 233, 32, 120, 46, 46]
binary/ljust9	ASCII-8BIT [99, 97, 102, 233, 32, 120, 46, 46, 46]
binary/rjust9	ASCII-8BIT [46, 46, 46, 99, 97, 102, 233, 32, 120]
binary/delete_x	ASCII-8BIT [99, 97, 102, 233, 32]
binary/squeeze	ASCII-8BIT [99, 97, 102, 233, 32, 120]
binary/tr	ASCII-8BIT [99, 97, 102, 233, 32, 122]
binary/chars	["ASCII-8BIT [99]", "ASCII-8BIT [97]", "ASCII-8BIT [102]", "ASCII-8BIT [233]", "ASCII-8BIT [32]", "ASCII-8BIT [120]"]
binary/codepoints	[99, 97, 102, 233, 32, 120]
koi8/strip	KOI8-R [193, 194, 32, 120]
koi8/lstrip	KOI8-R [193, 194, 32, 120]
koi8/rstrip	KOI8-R [193, 194, 32, 120]
koi8/chop	KOI8-R [193, 194, 32]
koi8/center9	KOI8-R [46, 46, 193, 194, 32, 120, 46, 46, 46]
koi8/ljust9	KOI8-R [193, 194, 32, 120, 46, 46, 46, 46, 46]
koi8/rjust9	KOI8-R [46, 46, 46, 46, 46, 193, 194, 32, 120]
koi8/delete_x	KOI8-R [193, 194, 32]
koi8/squeeze	KOI8-R [193, 194, 32, 120]
koi8/tr	KOI8-R [193, 194, 32, 122]
koi8/chars	["KOI8-R [193]", "KOI8-R [194]", "KOI8-R [32]", "KOI8-R [120]"]
koi8/codepoints	[193, 194, 32, 120]
eucjp/strip	EUC-JP [164, 162, 164, 164, 32, 120]
eucjp/lstrip	EUC-JP [164, 162, 164, 164, 32, 120]
eucjp/rstrip	EUC-JP [164, 162, 164, 164, 32, 120]
eucjp/chop	EUC-JP [164, 162, 164, 164, 32]
eucjp/center9	EUC-JP [46, 46, 164, 162, 164, 164, 32, 120, 46, 46, 46]
eucjp/ljust9	EUC-JP [164, 162, 164, 164, 32, 120, 46, 46, 46, 46, 46]
eucjp/rjust9	EUC-JP [46, 46, 46, 46, 46, 164, 162, 164, 164, 32, 120]
eucjp/delete_x	EUC-JP [164, 162, 164, 164, 32]
eucjp/squeeze	EUC-JP [164, 162, 164, 164, 32, 120]
eucjp/tr	EUC-JP [164, 162, 164, 164, 32, 122]
eucjp/chars	["EUC-JP [164, 162]", "EUC-JP [164, 164]", "EUC-JP [32]", "EUC-JP [120]"]
eucjp/codepoints	[42146, 42148, 32, 120]
latin1/sub	ISO-8859-1 [99, 97, 102, 233, 32, 121]
latin1/gsub	ISO-8859-1 [99, 97, 102, 233, 32, 121]
latin1/sub_re	ISO-8859-1 [99, 97, 102, 233, 32, 121]
latin1/split_sp	["ISO-8859-1 [99, 97, 102, 233]", "ISO-8859-1 [120]"]
latin1/split_empty	["ISO-8859-1 [99]", "ISO-8859-1 [97]", "ISO-8859-1 [102]", "ISO-8859-1 [233]", "ISO-8859-1 [32]", "ISO-8859-1 [120]"]
latin1/scan	["ISO-8859-1 [120]"]
binary/sub	ASCII-8BIT [99, 97, 102, 233, 32, 121]
binary/gsub	ASCII-8BIT [99, 97, 102, 233, 32, 121]
binary/sub_re	ASCII-8BIT [99, 97, 102, 233, 32, 121]
binary/split_sp	["ASCII-8BIT [99, 97, 102, 233]", "ASCII-8BIT [120]"]
binary/split_empty	["ASCII-8BIT [99]", "ASCII-8BIT [97]", "ASCII-8BIT [102]", "ASCII-8BIT [233]", "ASCII-8BIT [32]", "ASCII-8BIT [120]"]
binary/scan	["ASCII-8BIT [120]"]
koi8/sub	KOI8-R [193, 194, 32, 121]
koi8/gsub	KOI8-R [193, 194, 32, 121]
koi8/sub_re	KOI8-R [193, 194, 32, 121]
koi8/split_sp	["KOI8-R [193, 194]", "KOI8-R [120]"]
koi8/split_empty	["KOI8-R [193]", "KOI8-R [194]", "KOI8-R [32]", "KOI8-R [120]"]
koi8/scan	["KOI8-R [120]"]
eucjp/sub	EUC-JP [164, 162, 164, 164, 32, 121]
eucjp/gsub	EUC-JP [164, 162, 164, 164, 32, 121]
eucjp/sub_re	EUC-JP [164, 162, 164, 164, 32, 121]
eucjp/split_sp	["EUC-JP [164, 162, 164, 164]", "EUC-JP [120]"]
eucjp/split_empty	["EUC-JP [164, 162]", "EUC-JP [164, 164]", "EUC-JP [32]", "EUC-JP [120]"]
eucjp/scan	["EUC-JP [120]"]
latin1/match0	ISO-8859-1 [120]
latin1/match_pre	ISO-8859-1 [99, 97, 102, 233, 32]
koi8/match0	KOI8-R [120]
koi8/match_pre	KOI8-R [193, 194, 32]
eucjp/match0	EUC-JP [120]
eucjp/match_pre	EUC-JP [164, 162, 164, 164, 32]

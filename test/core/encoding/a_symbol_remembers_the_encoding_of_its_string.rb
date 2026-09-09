# `str.to_sym.to_s` answers that encoding and those bytes, because interning is
# by bytes plus encoding.
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

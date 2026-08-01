# `Encoding.compatible?` over the shapes that decide it: which side is empty,
# which side is 7-bit, and whether either encoding is ascii-compatible. The
# order the two sides are asked in IS the answer -- two ascii-only strings
# take the FIRST one's encoding, and an empty second side always yields.

A = {
  "u8-ascii"  => "abc".dup.force_encoding("UTF-8"),
  "u8-wide"   => "é".dup.force_encoding("UTF-8"),
  "u8-empty"  => "".dup.force_encoding("UTF-8"),
  "l1-ascii"  => "abc".dup.force_encoding("ISO-8859-1"),
  "l1-high"   => "\xe9".dup.force_encoding("ISO-8859-1"),
  "l1-empty"  => "".dup.force_encoding("ISO-8859-1"),
  "us-ascii"  => "abc".dup.force_encoding("US-ASCII"),
  "u8-broken" => "\xff".dup.force_encoding("UTF-8"),
  "16be"      => "\x00a".dup.force_encoding("UTF-16BE"),
  "16be-empty"=> "".dup.force_encoding("UTF-16BE"),
}
A.each do |ka, a|
  A.each do |kb, b|
    r = Encoding.compatible?(a, b)
    puts "#{ka}\t#{kb}\t#{r ? r.name : "nil"}"
  end
end

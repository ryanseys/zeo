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
__END__
u8-ascii	u8-ascii	UTF-8
u8-ascii	u8-wide	UTF-8
u8-ascii	u8-empty	UTF-8
u8-ascii	l1-ascii	UTF-8
u8-ascii	l1-high	ISO-8859-1
u8-ascii	l1-empty	UTF-8
u8-ascii	us-ascii	UTF-8
u8-ascii	u8-broken	UTF-8
u8-ascii	16be	nil
u8-ascii	16be-empty	UTF-8
u8-wide	u8-ascii	UTF-8
u8-wide	u8-wide	UTF-8
u8-wide	u8-empty	UTF-8
u8-wide	l1-ascii	UTF-8
u8-wide	l1-high	nil
u8-wide	l1-empty	UTF-8
u8-wide	us-ascii	UTF-8
u8-wide	u8-broken	UTF-8
u8-wide	16be	nil
u8-wide	16be-empty	UTF-8
u8-empty	u8-ascii	UTF-8
u8-empty	u8-wide	UTF-8
u8-empty	u8-empty	UTF-8
u8-empty	l1-ascii	UTF-8
u8-empty	l1-high	ISO-8859-1
u8-empty	l1-empty	UTF-8
u8-empty	us-ascii	UTF-8
u8-empty	u8-broken	UTF-8
u8-empty	16be	UTF-16BE
u8-empty	16be-empty	UTF-8
l1-ascii	u8-ascii	ISO-8859-1
l1-ascii	u8-wide	UTF-8
l1-ascii	u8-empty	ISO-8859-1
l1-ascii	l1-ascii	ISO-8859-1
l1-ascii	l1-high	ISO-8859-1
l1-ascii	l1-empty	ISO-8859-1
l1-ascii	us-ascii	ISO-8859-1
l1-ascii	u8-broken	UTF-8
l1-ascii	16be	nil
l1-ascii	16be-empty	ISO-8859-1
l1-high	u8-ascii	ISO-8859-1
l1-high	u8-wide	nil
l1-high	u8-empty	ISO-8859-1
l1-high	l1-ascii	ISO-8859-1
l1-high	l1-high	ISO-8859-1
l1-high	l1-empty	ISO-8859-1
l1-high	us-ascii	ISO-8859-1
l1-high	u8-broken	nil
l1-high	16be	nil
l1-high	16be-empty	ISO-8859-1
l1-empty	u8-ascii	ISO-8859-1
l1-empty	u8-wide	UTF-8
l1-empty	u8-empty	ISO-8859-1
l1-empty	l1-ascii	ISO-8859-1
l1-empty	l1-high	ISO-8859-1
l1-empty	l1-empty	ISO-8859-1
l1-empty	us-ascii	ISO-8859-1
l1-empty	u8-broken	UTF-8
l1-empty	16be	UTF-16BE
l1-empty	16be-empty	ISO-8859-1
us-ascii	u8-ascii	US-ASCII
us-ascii	u8-wide	UTF-8
us-ascii	u8-empty	US-ASCII
us-ascii	l1-ascii	US-ASCII
us-ascii	l1-high	ISO-8859-1
us-ascii	l1-empty	US-ASCII
us-ascii	us-ascii	US-ASCII
us-ascii	u8-broken	UTF-8
us-ascii	16be	nil
us-ascii	16be-empty	US-ASCII
u8-broken	u8-ascii	UTF-8
u8-broken	u8-wide	UTF-8
u8-broken	u8-empty	UTF-8
u8-broken	l1-ascii	UTF-8
u8-broken	l1-high	nil
u8-broken	l1-empty	UTF-8
u8-broken	us-ascii	UTF-8
u8-broken	u8-broken	UTF-8
u8-broken	16be	nil
u8-broken	16be-empty	UTF-8
16be	u8-ascii	nil
16be	u8-wide	nil
16be	u8-empty	UTF-16BE
16be	l1-ascii	nil
16be	l1-high	nil
16be	l1-empty	UTF-16BE
16be	us-ascii	nil
16be	u8-broken	nil
16be	16be	UTF-16BE
16be	16be-empty	UTF-16BE
16be-empty	u8-ascii	UTF-8
16be-empty	u8-wide	UTF-8
16be-empty	u8-empty	UTF-16BE
16be-empty	l1-ascii	ISO-8859-1
16be-empty	l1-high	ISO-8859-1
16be-empty	l1-empty	UTF-16BE
16be-empty	us-ascii	US-ASCII
16be-empty	u8-broken	UTF-8
16be-empty	16be	UTF-16BE
16be-empty	16be-empty	UTF-16BE

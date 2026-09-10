# Integer#to_s on a Bignum answers an ordinary String, so it must live on the
# string heap with its marker byte.
#
# A bare allocation is not enough: a string reports its length from a header
# the allocator does not write, so a buffer without one reports whatever
# happened to be in memory, and the next concat copies that many bytes. Under
# GC stress that
# is a heap-buffer-overflow inside sp_str_concat; the visible symptom is a SEGV
# in memcpy with a concat on the stack.
#
# Interleaving other allocation is the point: it decides what sits before the
# chunk, which is what the misread length came from.

big = 2**200
noise = []

s = big.to_s
p s.length
p s[0, 3]
p s.bytesize

# through concat, the shape that overran
20.times do |i|
  noise.push("filler-#{i}-#{'x' * i}")
  t = (2**200 + i).to_s + "-" + (2**64 + i).to_s
  p t if i == 19
end

# the same value reached through the poly path (a container read is poly)
mixed = [big, "s"]
p mixed[0].to_s.length
p mixed[0].to_s + "!"

# a BigRational renders by concatenating two of these
r = Rational(2**100, 3)
p r.to_s
p r.inspect

# and the small-value fast path inside the same function
p (2**64).to_s
p (-(2**64)).to_s
p noise.length
__END__
61
"160"
61
"1606938044258990275541962092341162602522202993782792835301395-18446744073709551635"
61
"1606938044258990275541962092341162602522202993782792835301376!"
"1267650600228229401496703205376/3"
"(1267650600228229401496703205376/3)"
"18446744073709551616"
"-18446744073709551616"
20

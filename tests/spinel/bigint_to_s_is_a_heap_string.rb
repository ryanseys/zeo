# Integer#to_s on a Bignum answers an ordinary String, so it must live on the
# string heap with its marker byte.
#
# It used to be a bare malloc'd buffer. Nothing reads the byte before such a
# chunk deliberately, but sp_str_byte_len does -- it is how every spinel string
# reports its length -- so the length came out of whatever the allocator had
# put there, and the next concat memcpy'd that many bytes. Under GC stress that
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

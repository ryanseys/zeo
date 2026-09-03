# The critical semantic behind `Gem::SecureRandom`: a host defines its own
# `gen_random` leaf, extends the native formatter, and the formatter's
# `hex`/`random_bytes` redispatch back to that leaf -- so a DETERMINISTIC
# leaf makes the formatted output deterministic and testable exactly.

require "random/formatter"
# `Random::Formatter` is a NATIVE module, so it must be mixed in with the
# runtime `extend` call (the in-body directive only re-materializes a
# user module's own Ruby methods).
module FixedSource
  def self.gen_random(n); ("\xAB".b * n); end
end
FixedSource.extend(Random::Formatter)
module FixedSource2
  def self.gen_random(n); ("\x00".b * n); end
end
FixedSource2.extend(Random::Formatter)

puts FixedSource.hex(4)                 # "abababab"
puts FixedSource.random_bytes(3).bytes.inspect
puts FixedSource2.hex(4)                # "00000000"
puts FixedSource2.random_number(256)    # first byte of 0x00.. => 0
__END__
abababab
[171, 171, 171]
00000000
0

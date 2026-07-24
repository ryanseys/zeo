# String#sum(bits): byte checksum modulo 2**bits (default 16; <= 0 or
# >= 64 untruncated like CRuby).
puts "hello".sum(8)
p "abc".sum
p "hello".sum(0)
p "hi".sum(64)

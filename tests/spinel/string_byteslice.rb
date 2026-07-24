# String#byteslice: byte-indexed substring. Negative start counts from
# the end. (Out-of-range yields "" like slice, not CRuby's nil, so it is
# not exercised here.)
puts "hello".byteslice(0, 3)
puts "hello".byteslice(1, 3)
puts "hello".byteslice(-2, 1)
puts "hello".byteslice(1)
puts "hello".byteslice(0, 5)
puts "hello".byteslice(2, 100)

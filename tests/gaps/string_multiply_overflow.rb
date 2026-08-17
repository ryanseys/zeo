# GAP -- imported from the spinel corpus at c55d9bdb.
# String#* overflow raises "string size too big"; ruby 4.0.6 says "argument
# too big".
#
# Issue #836: String * <huge> raises ArgumentError instead of
# segfaulting (the implicit malloc-NULL + memcpy chain).
#
# The receiver is 1000 bytes, not 1, so `len * times` overflows the size check
# and raises before anything is allocated. A 1-byte receiver takes the ALLOCATE
# path instead: ruby 4.0.6 answers NoMemoryError there, and which of the two
# NoMemoryError messages it picks depends on the machine -- a golden that
# travels has to stay on the arithmetic side of the guard.
begin
  puts ("x" * 1000) * (1 << 60)
rescue ArgumentError => e
  puts "huge: " + e.message
end
begin
  puts "x" * -1
rescue ArgumentError => e
  puts "neg: " + e.message
end
puts "ab" * 3
puts ("hi" * 0).inspect

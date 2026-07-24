# Object#object_id — unique stable id per object value.
# Int uses the MRI tagged formula (2*n+1), string uses the pointer
# bit pattern, symbol uses its interned id scaled to avoid collision.
# The exact numbers aren't part of the contract; spinel just needs
# values that are stable and unique-ish.

puts 42.object_id == 42.object_id   # true (same value, same id)
puts 1.object_id != 2.object_id     # true (different value)

s = "hello"
puts s.object_id == s.object_id     # true (same object)

puts :foo.object_id == :foo.object_id  # true (interned)
puts :foo.object_id != :bar.object_id  # true (different sym)

# Returns Integer
puts 42.object_id.class
puts "x".object_id.class
puts :s.object_id.class

# Object#object_id. The contract is stability and distinctness, not any
# particular number: one object answers the same id every time, and two
# different objects answer different ids. The program compares ids to each
# other, never to a literal.

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
__END__
true
true
true
true
true
Integer
Integer
Integer

# frozen_string_literal: true
# Under the `frozen_string_literal: true` pragma above, a mutating method on
# a string literal raises FrozenError. A string built with String.new is not
# a literal, so it still mutates.
begin
  "hello".insert(0, "X")
  puts "BUG: insert no raise"
rescue FrozenError => e
  puts "insert: " + e.message
end
begin
  "hello".prepend("X")
  puts "BUG: prepend no raise"
rescue FrozenError => e
  puts "prepend: " + e.message
end
begin
  "hello" << "Y"
  puts "BUG: << no raise"
rescue FrozenError => e
  puts "<<: " + e.message
end

# Mutable strings still work.
s = String.new("hi")
s << "!"
puts s
__END__
insert: can't modify frozen String: "hello"
prepend: can't modify frozen String: "hello"
<<: can't modify frozen String: "hello"
hi!

# Ruby forbids adding a new key to a Hash while iterating it: RuntimeError
# "can't add a new key into hash during iteration". zeo's Hash#each takes no
# iteration borrow, so the insert silently succeeds. Updating an EXISTING key
# stays legal on both sides (the control below).
h = { a: 1, b: 2 }
begin
  h.each { |k, v| h[:c] = 3 }
rescue RuntimeError => e
  puts "insert: #{e.message}"
end
puts h.key?(:c).inspect

h2 = { a: 1 }
h2.each { |k, v| h2[:a] = 99 }
puts h2[:a]

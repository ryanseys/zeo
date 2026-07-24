# Issue #914: Hash#fetch without a default raises KeyError on
# a missing key, not 0.
h = {a: 1}

# With a default — works as before.
puts h.fetch(:missing, "default")
puts h.fetch(:a)

# Without a default — should raise KeyError.
begin
  h.fetch(:missing)
  puts "no exception"
rescue KeyError
  puts "caught KeyError"
end

# String-keyed hash.
h2 = {"x" => 1}
begin
  h2.fetch("y")
rescue KeyError
  puts "str-hash caught"
end

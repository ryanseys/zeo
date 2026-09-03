# The table rows validate argument TYPES with CRuby's exact TypeError/
# ArgumentError messages (previously an arg-type mismatch degraded to
# NoMethodError) -- all real, rescuable exceptions now.

a = ["a"].first
begin
  a + 1
rescue TypeError => e
  puts "TypeError: #{e.message}"
end
arr = [[1]].first
begin
  arr[:x]
rescue TypeError => e
  puts "TypeError: #{e.message}"
end
one = [1].first
begin
  one < "a"
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end
__END__
TypeError: no implicit conversion of Integer into String
TypeError: no implicit conversion of Symbol into Integer
ArgumentError: comparison of Integer with String failed

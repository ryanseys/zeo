# Issue #847: String#index(nil) used to silently return -1 / nil,
# conflating "no implicit conversion" with "not found". MRI raises
# TypeError for nil arg.
begin
  "hello".index(nil)
  puts "BUG: no raise"
rescue TypeError => e
  puts "TypeError: " + e.message
end

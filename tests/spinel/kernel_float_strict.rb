# Issue #888: Kernel#Float() raises ArgumentError on unparseable
# input per MRI; previously silently returned 0.0 from strtod.
begin
  Float("abc")
rescue ArgumentError => e
  puts "abc: " + e.message
end
begin
  Float("")
rescue ArgumentError => e
  puts "empty: " + e.message
end
puts Float("3.14")
puts Float("  42.5  ")
puts Float(7)
puts Float(2.5)

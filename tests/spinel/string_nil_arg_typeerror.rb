# String methods that raise TypeError for nil arguments.
# Issue #847 covered index(); this extends to rindex,
# include?, start_with?, end_with?, and count.

begin
  "hello".rindex(nil)
  puts "BUG rindex: no raise"
rescue TypeError => e
  puts "rindex: #{e.message}"
end

begin
  "hello".include?(nil)
  puts "BUG include?: no raise"
rescue TypeError => e
  puts "include?: #{e.message}"
end

begin
  "hello".start_with?(nil)
  puts "BUG start_with?: no raise"
rescue TypeError => e
  puts "start_with?: #{e.message}"
end

begin
  "hello".end_with?(nil)
  puts "BUG end_with?: no raise"
rescue TypeError => e
  puts "end_with?: #{e.message}"
end

begin
  "hello".count(nil)
  puts "BUG count: no raise"
rescue TypeError => e
  puts "count: #{e.message}"
end

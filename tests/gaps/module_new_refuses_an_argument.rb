begin
  m = Module.new(1)
  puts "ok #{m.class}"
rescue ArgumentError => e
  puts "ArgumentError: #{e.message}"
end

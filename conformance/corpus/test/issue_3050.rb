r = Random.new(42)
puts r.bytes(0).bytesize
begin
  r.bytes(-1)
rescue ArgumentError => e
  puts "ArgumentError"
  puts e.message
end
begin
  Random.bytes(-5)
rescue ArgumentError
  puts "class ok"
end

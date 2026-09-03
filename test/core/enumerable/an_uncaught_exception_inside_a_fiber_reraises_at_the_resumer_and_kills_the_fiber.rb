x = Fiber.new do
  raise "boom in fiber"
end
begin
  x.resume
rescue RuntimeError => e
  puts "caught: #{e.send(:message)}"
end
puts x.alive?
__END__
caught: boom in fiber
false

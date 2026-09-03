mu = Mutex.new
r = mu.synchronize { 42 }
puts r
puts mu.locked?
begin
  mu.synchronize { raise "inside" }
rescue RuntimeError => e
  puts "rescued: #{e.send(:message)}"
end
puts mu.locked?
__END__
42
false
rescued: inside
false

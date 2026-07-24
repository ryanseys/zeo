s = "hello".freeze
puts s.frozen?
begin
  s << " world"
rescue FrozenError
  puts "<<: raised"
end
begin
  s.insert(0, "X")
rescue FrozenError
  puts "insert: raised"
end
begin
  s.prepend("X")
rescue FrozenError
  puts "prepend: raised"
end
begin
  s.replace("X")
rescue FrozenError
  puts "replace: raised"
end
puts s

s2 = "hello".dup
puts s2.frozen?
s2 << " world"
puts s2

s3 = String.new("abc")
puts s3.frozen?
s3.freeze
puts s3.frozen?
begin
  s3 << "d"
rescue FrozenError
  puts "post-freeze: raised"
end

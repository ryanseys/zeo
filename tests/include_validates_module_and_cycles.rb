# `include` validates: a Class argument is a TypeError ("wrong argument
# type Class (expected Module)") and a cyclic or self include is an
# ArgumentError ("cyclic include detected"). zeo accepts all three
# silently. (Found by the 2026-08-24 probe sweep.)
begin
  Class.new.include(Class.new)
  puts "class included"
rescue TypeError => e
  puts e.message
end
m = Module.new
n = Module.new { include m }
begin
  m.include(n)
  puts "cycle included"
rescue ArgumentError => e
  puts e.message
end
begin
  m.include(m)
  puts "self included"
rescue ArgumentError => e
  puts e.message
end

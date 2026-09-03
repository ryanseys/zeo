# The fallible-display contract: a user `to_s`/`inspect` that raises
# surfaces as a CATCHABLE exception from puts/print/p/warn/format/
# interpolation (string and regexp)/Array+Hash to_s -- never a
# runtime panic. Partial-output rules are CRuby's own: puts/print/p
# flush what rendered before the raise, warn flushes nothing.
# Expected output is verbatim ruby 4.0.6.

class Boom
  def to_s
    raise "to_s boom"
  end
  def inspect
    raise "inspect boom"
  end
end
b = Boom.new
begin
  puts b
rescue => e
  puts "puts: #{e.class}: #{e.message}"
end
begin
  print b
rescue => e
  puts "print: #{e.class}: #{e.message}"
end
begin
  x = "v=#{b}"
rescue => e
  puts "interp: #{e.class}: #{e.message}"
end
begin
  p b
rescue => e
  puts "p: #{e.class}: #{e.message}"
end
begin
  s = format("%s", b)
rescue => e
  puts "format s: #{e.class}: #{e.message}"
end
begin
  s = format("%p", b)
rescue => e
  puts "format p: #{e.class}: #{e.message}"
end
begin
  puts [1, b, 2]
rescue => e
  puts "puts arr: #{e.class}: #{e.message}"
end
begin
  p [1, b]
rescue => e
  puts "p arr: #{e.class}: #{e.message}"
end
begin
  puts({ k: b }.to_s)
rescue => e
  puts "hash to_s: #{e.class}: #{e.message}"
end
puts "still running"
begin
  p 1, b
rescue => e
  puts "p multi: #{e.class}: #{e.message}"
end
begin
  print "x", b
rescue => e
  puts "print multi: #{e.class}: #{e.message}"
end
puts
begin
  warn "w1", b
rescue => e
  puts "warn multi: #{e.class}: #{e.message}"
end
begin
  s = "pat#{b}"
rescue => e
  puts "interp2: #{e.class}: #{e.message}"
end
begin
  r = /x#{b}/
rescue => e
  puts "regexp interp: #{e.class}: #{e.message}"
end
puts "end"
__END__
puts: RuntimeError: to_s boom
print: RuntimeError: to_s boom
interp: RuntimeError: to_s boom
p: RuntimeError: inspect boom
format s: RuntimeError: to_s boom
format p: RuntimeError: inspect boom
1
puts arr: RuntimeError: to_s boom
p arr: RuntimeError: inspect boom
hash to_s: RuntimeError: inspect boom
still running
1
p multi: RuntimeError: inspect boom
xprint multi: RuntimeError: to_s boom

warn multi: RuntimeError: to_s boom
interp2: RuntimeError: to_s boom
regexp interp: RuntimeError: to_s boom
end

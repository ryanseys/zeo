r = %r{a/b}
puts r.source
puts r.inspect
puts r.to_s
r2 = /foo\/bar/
puts r2.inspect
r3 = %r{a/b/c}mi
puts r3.inspect
puts r3.to_s
puts(/x/.inspect)

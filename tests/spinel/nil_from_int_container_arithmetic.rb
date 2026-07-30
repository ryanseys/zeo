# A miss on an Integer-element Hash or Array is nil, so arithmetic on it
# raises rather than computing on the sentinel that represents it: nil on the
# left is NoMethodError, nil on the right is the coercion TypeError, exactly
# as CRuby reports them. Reading the value and asking nil? still work.
h = { 0 => 0 }
a = [1, 2, 3]

v = h[9]
p v.nil?
p v.inspect
p v.class

begin
  p(h[9] + 1)
rescue NoMethodError => e
  puts "#{e.class}: #{e.message}"
end

begin
  p(a[99] - 1)
rescue NoMethodError => e
  puts "#{e.class}: #{e.message}"
end

begin
  p(1 + h[9])
rescue TypeError => e
  puts "#{e.class}: #{e.message}"
end

begin
  p(a[99] * 2)
rescue NoMethodError => e
  puts "#{e.class}: #{e.message}"
end

begin
  p(h[9] / 2)
rescue NoMethodError => e
  puts "#{e.class}: #{e.message}"
end

begin
  p(a[99] % 2)
rescue NoMethodError => e
  puts "#{e.class}: #{e.message}"
end

begin
  p h[9].abs
rescue NoMethodError => e
  puts "#{e.class}: #{e.message}"
end

# a present key still computes
p h[0] + 1
p a[0] * 3
p a[-1] - 1

# and the usual nil-guarding idioms are unaffected
p(h[9] || 7)
p(h.fetch(9, 7) + 1)
p(a[99].nil? ? 0 : 1)

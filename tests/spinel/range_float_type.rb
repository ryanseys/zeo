# Distinct Float range type (1.0..3.0)
r = (1.0..3.0)
p r
p r.begin
p r.end
p r.first
p r.last
p r.min
p r.max
p r.include?(2.0)
p r.include?(5.0)
p r.include?(0.0)
p r.cover?(1.5)
p r.cover?(3.0)
p r.exclude_end?
p r === 2.5
p r === 3.5
p (1.0...3.0).exclude_end?
p (1.0...3.0).cover?(3.0)
p r == (1.0..3.0)
p r == (1.0..4.0)
p r.eql?(1.0..3.0)
p r.to_s
p r.inspect
puts r
p r.step(0.5).to_a
p (0.0..1.0).step(0.25).to_a
p (3.0..1.0).step(0.5).to_a
p (1.0..3.0).class
p r.frozen?

# exclusive max raises
begin
  (1.0...3.0).max
rescue TypeError => e
  puts "max-excl: TypeError"
end

# iteration raises
begin
  (1.0..3.0).to_a
rescue TypeError => e
  puts "to_a: #{e.message}"
end
begin
  (1.0..3.0).each { |x| p x }
rescue TypeError => e
  puts "each: #{e.message}"
end

# bsearch bisects the reals
p (0.0..10.0).bsearch { |x| x >= 3.5 }

# case/when membership
case 2.5
when 1.0..3.0 then puts "in"
else puts "out"
end
case 9.9
when 1.0..3.0 then puts "in"
else puts "out"
end

# an ordinary int range is unaffected
ir = (1..3)
p ir
p ir.to_a
p ir.include?(2)

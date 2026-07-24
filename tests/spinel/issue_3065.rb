def t
  yield
rescue RangeError => e
  e.message
end
puts t { (1..).max }
puts t { (1...).max }
puts t { (..5).min }
puts((1..).min)
r = (10..)
puts t { r.max }

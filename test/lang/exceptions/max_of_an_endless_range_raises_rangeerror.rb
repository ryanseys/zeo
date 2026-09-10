# (1..).max is a RangeError, and the message says so.
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
__END__
cannot get the maximum of endless range
cannot get the maximum of endless range
cannot get the minimum of beginless range
1
cannot get the maximum of endless range

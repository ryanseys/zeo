# `#max` on an endless range and `#min` on a beginless range have no
# answer -- CRuby raises RangeError instead of iterating (which would loop
# forever). `(1..).min` still returns the begin (an endless range has a
# minimum). Regression: these used to hang the program.

def t; yield; rescue RangeError => e; e.message; end
puts t { (1..).max }
puts t { (1...).max }
puts t { (1.0..).max }
puts t { (..5).min }
puts((1..).min)
__END__
cannot get the maximum of endless range
cannot get the maximum of endless range
cannot get the maximum of endless range
cannot get the minimum of beginless range
1

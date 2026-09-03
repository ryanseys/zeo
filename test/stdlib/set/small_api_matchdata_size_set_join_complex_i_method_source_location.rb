# Small API wins: MatchData#size/#length, Set#join, the Complex::I
# imaginary-unit constant (and that it multiplies to -1), and Method's
# source_location/super_method (nil for a builtin, matching CRuby's C-method).

require "set"
m = "2026-06".match(/(\d+)-(\d+)/)
p m.size
p m.length
p Set[1, 2, 3].join("-")
p Complex::I
p(Complex::I * Complex::I)
p 5.method(:+).source_location
p 5.method(:+).super_method
__END__
3
3
"1-2-3"
(0+1i)
(-1+0i)
nil
nil

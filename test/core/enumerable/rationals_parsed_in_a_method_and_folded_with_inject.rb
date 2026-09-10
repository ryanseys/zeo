# A method splitting "1/2" into a Rational, called from each and from map, and
# the mapped values folded with inject.
def to_frac(str)
  num, den = str.split("/").map(&:to_i)
  Rational(num, den || 1)
end
exprs = [["1/2", "+", "1/3"]]
exprs.each { |a, op, b| p to_frac(a) }
total = exprs.map { |a, _, _| to_frac(a) }.inject(Rational(0), :+)
p total
__END__
(1/2)
(1/2)

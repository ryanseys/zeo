P = Data.define(:x, :y)
p(P.new(1, 2).frozen?)
a = P.new(1, 2); p(a.frozen?)
__END__
true
true

# :name.to_proc.lambda? is true (CRuby). Hash#values_at routes each key
# through [], so a missing key yields the hash's default, not bare nil.

p :upcase.to_proc.lambda?
p ["x", "y"].map(&:upcase)
p Hash.new(0).values_at(:x, :y)
p({ a: 1 }.values_at(:a, :z))
__END__
true
["X", "Y"]
[0, 0]
[1, nil]

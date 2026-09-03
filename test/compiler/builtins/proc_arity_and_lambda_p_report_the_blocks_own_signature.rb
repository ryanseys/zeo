p ->(a, b) {}.arity
p ->(a, b = 1) {}.arity
p proc { |a, b = 1| }.arity
p proc { |a, *b| }.arity
p ->(a, b:) {}.arity
p ->(a, **k) {}.arity
p ->() {}.lambda?
p proc {}.lambda?
__END__
2
-2
1
-2
2
-2
true
false

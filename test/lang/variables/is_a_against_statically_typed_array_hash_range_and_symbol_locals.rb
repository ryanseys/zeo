arr = [1, 2, 3]
puts arr.is_a?(Array)
puts arr.is_a?(Object)
puts arr.is_a?(Hash)
h = {a: 1}
puts h.is_a?(Hash)
r = 1..5
puts r.is_a?(Range)
sym = :foo
puts sym.is_a?(Symbol)
__END__
true
true
false
true
true
true

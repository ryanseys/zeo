h = { a: 1 }
p h.itself
e = [1, 2, 3].each
p e.nil?
pr = ->(x) { x }
p pr.respond_to?(:call)
__END__
{a: 1}
false
true

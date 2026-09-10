# Random#rand and Kernel#rand answer a non-negative Integer for a bound too
# large for a machine word.
r = Random.new(5)
p r.rand(2 ** 70).class
p r.rand(2 ** 70) >= 0
p r.rand(100).class
p rand(2 ** 70).class
__END__
Integer
true
Integer
Integer

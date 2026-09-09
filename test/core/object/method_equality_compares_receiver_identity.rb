# Two Methods over two distinct but equal Strings are unequal, though both call
# the same method.
m1 = "abc".method(:upcase)
m2 = "abc".method(:upcase)
p m1 == m2
__END__
false

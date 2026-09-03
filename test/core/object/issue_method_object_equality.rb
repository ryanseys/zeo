# Method#== compares receiver IDENTITY (`equal?`) as well as the bound method
# entry, so two Methods over two distinct String receivers are unequal even
# though both call the same String#upcase.
m1 = "abc".method(:upcase)
m2 = "abc".method(:upcase)
p m1 == m2
__END__
false

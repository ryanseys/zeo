# Method#== should compare receiver identity (equal?) as well as the bound
# method entry -- two Method objects bound to two distinct (non-identical)
# String receivers should be unequal even though both call the same
# String#upcase. zeo treats them as equal.
m1 = "abc".method(:upcase)
m2 = "abc".method(:upcase)
p m1 == m2

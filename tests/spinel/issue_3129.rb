b = 2 ** 80
p b[3]
p b[79]
p b.coerce(5).class
p b.clamp(0, 10)
p (2 ** 70).clamp(0, 2 ** 90).class
p (2 ** 80).coerce(5)
p (-(2 ** 80))[0]

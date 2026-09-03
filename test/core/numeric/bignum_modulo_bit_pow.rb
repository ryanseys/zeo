# Bignum modulo / % / remainder / divmod / #[] / modular pow (#2594).
b = 2 ** 100
p b.modulo(7)
p((-b).modulo(7))
p b.remainder(7)
p((-b).remainder(7))
p b.divmod(7)
p b[100]
p b[0]
p b.pow(2, 7)
p b % 7
__END__
2
5
2
-2
[181092942889747057356671886482, 2]
1
0
4
2

# pow with a modulus, digits, gcd, lcm, clamp, coerce and bit read, each given
# a number too large for a machine word.
# (spinel issue #3006)
b = 2 ** 70
p 5.pow(3, b).class
p 255.digits(b).class
p 12.gcd(b).class
p 12.lcm(b).class
p 5.clamp(0, b)
p 5.coerce(b).class
p 255[b]
p 1.step(b, 10 ** 20).first(2)
p 1.step(10, 3).to_a
__END__
Integer
Array
Integer
Integer
5
Array
0
[1, 100000000000000000001]
[1, 4, 7, 10]

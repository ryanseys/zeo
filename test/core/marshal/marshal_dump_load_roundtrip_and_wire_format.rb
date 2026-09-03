# Marshal round-trips the value tower (primitives, bignum, array, hash,
# Rational, shared refs and cycles) and writes CRuby's exact wire bytes for
# symbols (with ;-symlinks) and floats.

def rt(x) = Marshal.load(Marshal.dump(x))
p rt(42)
p rt(-987654321)
p rt(3.14)
p rt("hi")
p rt(:sym)
p rt([1, "x", :y, nil, true])
p rt(2 ** 200)
p rt(Rational(3, 4))
p [rt(Rational(1, 3)), rt(Rational(2, 5))]
shared = [1, 2]
sg = rt([shared, shared])
sg[0] << 99
p sg[1]
cyc = [10]
cyc << cyc
dc = rt(cyc)
p dc[1][1][0]
puts Marshal.dump([:ab, :cd, :ab]).bytes.join(",")
puts Marshal.dump(100.0).bytes.join(",")
__END__
42
-987654321
3.14
"hi"
:sym
[1, "x", :y, nil, true]
1606938044258990275541962092341162602522202993782792835301376
(3/4)
[(1/3), (2/5)]
[1, 2, 99]
10
4,8,91,8,58,7,97,98,58,7,99,100,59,0
4,8,102,8,49,101,50

r1 = ((1.0..10.0).step("x").to_a rescue $!.message)
p r1
r2 = ((1.0..10.0).step(:s).to_a rescue $!.message)
p r2
r3 = ((1.0..10.0).step(nil).to_a rescue $!.message)
p r3
r4 = ((1.0..10.0).step([1]).to_a rescue $!.message)
p r4
r5 = ((1.0..10.0).step(true).to_a rescue $!.message)
p r5

s1 = (("a".."e").step(:s).to_a rescue $!.message)
p s1
s2 = (("a".."e").step(nil).to_a rescue $!.message)
p s2
s3 = (("a".."e").step([1]).to_a rescue $!.message)
p s3
s4 = (("a".."e").step(true).to_a rescue $!.message)
p s4

i1 = ((1..10).step("x").to_a rescue $!.message)
p i1
i2 = ((1..10).step(:s).to_a rescue $!.message)
p i2
i3 = ((1..10).step(nil).to_a rescue $!.message)
p i3
i4 = ((1..10).step(true).to_a rescue $!.message)
p i4

b = "x"
i5 = ((1..10).step(b).to_a rescue $!.message)
p i5
i6 = ((1.0..9.0).step(b).to_a rescue $!.message)
p i6

p((1.0..3.0).step(0.5).to_a)
p((1..10).step(2).to_a)
p(("a".."e").step(2).to_a)
n = 2
p((1..10).step(n).to_a)
f = 0.5
p((1.0..3.0).step(f).to_a)
(1..6).step(2) { |x| p x }
(1.0..3.0).step(0.5) { |x| p x }
__END__
"String can't be coerced into Float"
":s can't be coerced into Float"
"nil can't be coerced into Float"
"Array can't be coerced into Float"
"true can't be coerced into Float"
"no implicit conversion of Symbol into String"
"no implicit conversion of nil into String"
"no implicit conversion of Array into String"
"no implicit conversion of true into String"
"String can't be coerced into Integer"
":s can't be coerced into Integer"
"nil can't be coerced into Integer"
"true can't be coerced into Integer"
"String can't be coerced into Integer"
"String can't be coerced into Float"
[1.0, 1.5, 2.0, 2.5, 3.0]
[1, 3, 5, 7, 9]
["a", "c", "e"]
[1, 3, 5, 7, 9]
[1.0, 1.5, 2.0, 2.5, 3.0]
1
3
5
1.0
1.5
2.0
2.5
3.0

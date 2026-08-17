# A counter the analysis widens to Bignum (repeated doubling) still has to
# work where an integer is wanted: an index, a count, a slice bound.
n = 10
a = Array.new(n) { |i| i * 10 }
k = 1
while k < 4
  k *= 2
end
p k
p a[k]
p a[(3 + k) % n]
p a[k, 2]
p a.first(k)
s = "abcdefghij"
p s[k]
p s[k, 3]
acc = 0
k.times { |i| acc += i }
p acc
p Array.new(k, 7)

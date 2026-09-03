# A seed only means anything if it replays somebody else's numbers: a test
# fixture, a shuffled deck rebuilt from a log, a property-test seed pasted out
# of a failure report. So `Random.new(42)` here draws ruby's own MT19937
# sequence, read through ruby's own three draws -- masked rejection for an
# integer bound, a 53-bit pair for a Float, and a `[0, 1]` variant for an
# INCLUSIVE range, which is what lets such a range reach its own endpoint.

r = Random.new(42)
p r.rand(100)
p r.rand(100)
p r.rand
p Random.new(42).rand(1000000)

# --- every seed shape reaches the same generator -----------------------------
[0, 1, 7, 12345, 2**31, 2**32, 2**32 + 1, 2**64 + 7, 2**128, -99, 3.9].each do |s|
  p [s, Random.new(s).rand(1000)]
end

# --- bounds ------------------------------------------------------------------
r = Random.new(5)
p r.rand(1)
p r.rand(2)
p r.rand(2.5)
p r.rand(2..9)
p r.rand(2...9)
p r.rand(1.0..2.0)
p r.rand(1.0...2.0)
p Random.new(5).rand(2**80)
p Random.new(5).rand(10**30)
p Random.new(5).bytes(16).unpack1("H*")

# --- the process-wide generator, which `srand` reseeds -----------------------
srand(1234)
p rand(100)
p rand(100)
p rand
p rand(1..6)
p 10.times.map { rand(6) + 1 }
p srand(99).class
p rand(100)

# --- shuffle and sample draw from the same stream ----------------------------
p [1, 2, 3, 4, 5].shuffle(random: Random.new(42))
p (1..12).to_a.shuffle(random: Random.new(3))
p [1, 2, 3, 4, 5].sample(random: Random.new(42))
# Ruby has two ways of turning a draw into a distinct index, and switches
# between them at eleven picks; both sides of that line are pinned.
(1..14).each { |n| p (1..50).to_a.sample(n, random: Random.new(n)) }
p [].sample(random: Random.new(1))
p [].shuffle(random: Random.new(1))

# --- two generators on the same seed stay in step ----------------------------
a = Random.new(77)
b = Random.new(77)
p 5.times.map { a.rand(1000) } == 5.times.map { b.rand(1000) }
p Random.new(42) == Random.new(42)
p Random.new(42) == Random.new(43)
__END__
51
92
0.1834347898661638
121958
[0, 684]
[1, 37]
[7, 175]
[12345, 482]
[2147483648, 282]
[4294967296, 973]
[4294967297, 501]
[18446744073709551623, 264]
[340282366920938463463374607431768211456, 558]
[-99, 641]
[3.9, 874]
0
1
0.13795030993842133
7
8
1.36373689559256
1.979444999161446
658236186675747056275439
242056307132997733637495580349
"638bd438ce48200eef4fe8debde6d1d4"
47
83
0.6221087710398319
5
[5, 1, 2, 2, 2, 3, 4, 5, 5, 3]
Integer
1
[2, 5, 3, 1, 4]
[6, 5, 2, 3, 12, 7, 8, 1, 4, 10, 9, 11]
4
[38]
[41, 16]
[43, 25, 4]
[47, 6, 2, 43]
[36, 15, 50, 41, 18]
[11, 10, 38, 23, 47, 18]
[48, 5, 27, 4, 22, 28, 45]
[4, 22, 44, 7, 30, 11, 24, 48]
[29, 23, 2, 25, 45, 32, 39, 35, 34]
[10, 38, 17, 1, 32, 29, 35, 36, 11, 13]
[26, 18, 30, 21, 2, 5, 40, 15, 27, 34, 39]
[12, 29, 9, 6, 8, 3, 19, 30, 14, 23, 36, 46]
[19, 50, 13, 20, 43, 40, 32, 5, 35, 12, 48, 25, 23]
[44, 26, 15, 2, 12, 1, 49, 36, 19, 4, 37, 11, 8, 46]
nil
[]
true
true
false

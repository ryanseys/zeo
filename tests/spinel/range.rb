# Range methods — first / last / include? / to_a / each, cover? /
# min / max / count, Range#map, inclusive-vs-exclusive distinction,
# step without block param. Was five separate tests; merged here.
# No class collisions. The only top-level def (`classify` in the
# inclusive/exclusive section) is unique within the file. Local
# names use per-section prefixes where the original tests reused
# generic names like `r`, `sum`, `i`.

# === Basic Range methods ===
r0 = (1..10)
puts r0.first             # 1
puts r0.last              # 10
puts r0.include?(5)       # true
puts r0.include?(11)      # false

arr0 = r0.to_a
puts arr0.length          # 10
puts arr0.sum             # 55

total = 0
r0.each do |x|
  total += x
end
puts total                # 55
puts "done"

# === cover? / min / max / count ===
puts (1..10).cover?(5)
puts (1..10).cover?(1)
puts (1..10).cover?(10)
puts (1..10).cover?(0)
puts (1..10).cover?(11)
puts (-5..5).cover?(0)
puts (-5..5).cover?(-5)
puts (-5..5).cover?(6)

puts (1..10).min
puts (1..10).max
puts (-5..5).min
puts (-5..5).max
puts (100..200).min
puts (100..200).max

puts (1..10).count
puts (1..1).count
puts (-5..5).count
puts (100..200).count

puts (1...10).count
puts (1...1).count
puts (-5...5).count

# === Range#map ===
puts (0..3).map { |i| i * 10 }.length      # 4
puts (0..3).map { |i| i * 10 }[2]          # 20
puts (0...3).map { |i| i }.length          # 3
puts (0...3).map { |i| i }[2]              # 2
puts ((1..3).map { |i| "x#{i}" }.join(","))   # x1,x2,x3
puts ((0...3).map { |i| i * 0.5 }.length)     # 3

# === Inclusive vs exclusive (slicing / for-in / Range#to_a / case-when) ===
ia = [10, 20, 30, 40, 50]
puts ia[1..3].length      # 3 (inclusive)
puts ia[1...3].length     # 2 (exclusive)
puts ia[1...3][0]         # 20
puts ia[1...3][1]         # 30

sa = "a,b,c,d,e".split(",")
puts sa[0..2].join(":")   # a:b:c
puts sa[0...2].join(":")  # a:b

fa = [1.5, 2.5, 3.5, 4.5]
puts fa[0..2].length      # 3
puts fa[0...2].length     # 2
puts fa[0...3].sum        # 7.5

s = "abcdef"
puts s[1..3]              # bcd
puts s[1...3]             # bc

sum_inc = 0
for ix in 1..3
  sum_inc = sum_inc + ix
end
puts sum_inc              # 6

sum_exc = 0
for ix in 1...3
  sum_exc = sum_exc + ix
end
puts sum_exc              # 3

puts (1..3).to_a.length   # 3
puts (1...3).to_a.length  # 2

def classify(n)
  case n
  when 0...10 then "small"
  when 10..99 then "medium"
  else "large"
  end
end
puts classify(5)
puts classify(9)
puts classify(10)
puts classify(99)
puts classify(100)

# === Range#=== (case-when membership) ===
puts (1..10) === 5      # true
puts (1..10) === 1      # true
puts (1..10) === 10     # true
puts (1..10) === 0      # false
puts (1..10) === 11     # false

# === step without block param ===
# `Integer#step` with a do-block that omits its parameter used a
# synthesized `_i` index that wasn't declared. Two paramless `step`
# blocks in the same function also redefined it.
sum_step = 0
1.step(10, 1) do
  sum_step += 1
end
1.step(5, 1) do
  sum_step += 10
end
1.step(5, 1) do
  sum_step += 100
end
puts sum_step

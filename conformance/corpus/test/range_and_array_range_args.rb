# Cluster of "Range struct passed where mrb_int expected" C errors:
#   #1247 rand(range) / rand(float)
#   #1234 Range#cover? with a Range argument
#   #1228 Range#first(n) / #last(n) / #count
#   #1240 Array#values_at with a Range argument
#   #1224 Array#fill(value, range)
# Each used to either emit a C type error (sp_Range fed to an mrb_int
# operand) or silently return the wrong value.

# #1247 — rand with a range / float arg. Values are nondeterministic,
# so assert membership rather than the value.
vi = rand(1..6)
puts(vi >= 1 && vi <= 6)
puts vi.class
vf = rand(1.0..10.0)
puts(vf >= 1.0 && vf < 10.0)
puts vf.class

# #1234 — cover? with a Range argument
puts (1..10).cover?(5..8)
puts (1..10).cover?(5..12)
puts (1..10).cover?(5)

# #1228 — first(n) / last(n) / count
p (1..10).first(3)
p (1..10).last(3)
p (1...10).last(3)
p (1..10).first(20)
puts (1..10).count
puts (1...10).count

# #1240 — values_at with a Range argument
p [10, 20, 30, 40, 50].values_at(1..3)
p [10, 20, 30, 40, 50].values_at(0, 2, 4)
p ["a", "b", "c", "d"].values_at(1..2)

# #1224 — fill(value, range)
a = [1, 2, 3, 4, 5]
a.fill(0, 2..4)
p a
b = [1, 2, 3, 4, 5]
b.fill(9, 1...3)
p b

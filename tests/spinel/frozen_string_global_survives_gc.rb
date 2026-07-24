# A heap string frozen by .freeze and bound to a constant is rooted via
# sp_mark_user_globals, but the string-pool sweeper (sp_str_sweep) only kept
# 0xfc-marked strings and freed the frozen (0xf1) one -- a GC use-after-free
# on the next collect. Frozen globals must survive collection. (#1449)
VERSION = "0.8.0".freeze
NAME = ("spinel-" + "rt").freeze
GC.start
GC.start
sum = 0.0
i = 0
while i < 50000
  a = [1.0, 2.0, 3.0, 4.0]
  sum += a[0]
  i += 1
end
GC.start
puts VERSION
puts NAME
puts(sum > 0.0)

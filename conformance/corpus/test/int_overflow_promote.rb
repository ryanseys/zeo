# Existing bigint pattern detection catches `x *= y inside while`
# (and similar shapes); the result should be the full arbitrary-
# precision Ruby answer rather than a wrapped int64. Surface
# guard for the `sp_bigint_to_s` fast-path bug fixed alongside
# `--int-overflow=promote`: values just past 2^63 used to print
# wrapped because the fast path always cast through int64.
# Without the fix `2**63` printed as `-9223372036854775808` even
# though the bigint internally held the correct unsigned value.
#
# The linear-sum / non-multiplicative shapes that require the new
# `promote` mode to escape mrb_int width are exercised manually
# (the test harness here always builds with the raise/wrap
# helpers, not promote, so an analyzer-side rewrite of int
# locals isn't visible to it).

# 2**63 -- exact boundary where the old fast path wrapped.
m = 1
i = 0
while i < 63
  m = m * 2
  i = i + 1
end
puts m

# 2**100 -- deeper into multi-limb territory.
m = 1
i = 0
while i < 100
  m = m * 2
  i = i + 1
end
puts m

# Factorial of 25 -- classic bigint, well past int64.
n = 1
i = 1
while i <= 25
  n = n * i
  i = i + 1
end
puts n

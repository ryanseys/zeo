#
# `Float#round(n)` with a positive digit count rounds by scaling with a power
# of ten, so the SCALED product's own representation error becomes part of
# the value:
#
#   64.781995.round(5)   CRuby 64.782   zeo 64.78199
#
# `64.781995 * 1e5` is `6478199.4999999991`, so the digit that should round
# up rounds down. CRuby does not scale: `flo_round` works from the decimal
# representation for exactly this reason.
#
# The negative twin diverges the same way, and `round(2)`/`round(3)` agree --
# so this only shows at the digit counts where the scaled product lands near
# a half.
#
# The original header follows. It describes the PREDECESSOR project's
# version of this test and its own fix, not zeo's divergence above.
#
# Rounding at a positive digit count by scaling with a power of ten reads the
# scaled product's own representation error as part of the value: 64.781995 *
# 1e5 is 6478199.4999999991, so the digit that should round up rounds down and
# the answer comes out a decimal short. Each form compensates the way CRuby
# compensates, and the compensations differ per form on purpose.
vals = [44.781995, -44.781995, 64.781995, -64.781995, 2.675, -2.675, 1.005,
        0.5, 1.5, 2.5, -1.5, 65.1848, 32.669, -85320.29, 66471.846,
        123456789.987654321, 0.1, 1e-8, -1e-8]
vals.each do |v|
  p [v.round(5), v.round(2), v.floor(3), v.ceil(3), v.truncate(3),
     v.round(3, half: :even), v.round(3, half: :down), v.round(3, half: :up)]
end

# digit counts at and below zero answer an Integer, unchanged by any of this
p [1234.5678.round(0), 1234.5678.round(-2), (-1234.5678).round(-2),
   1234.5678.floor(-2), 1234.5678.ceil(-2), 1234.5678.truncate(-2)]

# a digit count only known at run time takes the same route
n = 5
p [64.781995.round(n), 64.781995.floor(n), 64.781995.ceil(n), 64.781995.truncate(n)]

# past the value's reach the value is answered unchanged
p [1.0e20.round(5), 1.0e-300.round(5), 0.0.round(5), (-0.0).round(5)]
__END__
[44.782, 44.78, 44.781, 44.782, 44.781, 44.782, 44.782, 44.782]
[-44.782, -44.78, -44.782, -44.781, -44.781, -44.782, -44.782, -44.782]
[64.782, 64.78, 64.781, 64.782, 64.781, 64.782, 64.782, 64.782]
[-64.782, -64.78, -64.782, -64.781, -64.781, -64.782, -64.782, -64.782]
[2.675, 2.68, 2.675, 2.675, 2.675, 2.675, 2.675, 2.675]
[-2.675, -2.68, -2.675, -2.675, -2.675, -2.675, -2.675, -2.675]
[1.005, 1.01, 1.005, 1.005, 1.005, 1.005, 1.005, 1.005]
[0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5]
[1.5, 1.5, 1.5, 1.5, 1.5, 1.5, 1.5, 1.5]
[2.5, 2.5, 2.5, 2.5, 2.5, 2.5, 2.5, 2.5]
[-1.5, -1.5, -1.5, -1.5, -1.5, -1.5, -1.5, -1.5]
[65.1848, 65.18, 65.184, 65.185, 65.184, 65.185, 65.185, 65.185]
[32.669, 32.67, 32.669, 32.669, 32.669, 32.669, 32.669, 32.669]
[-85320.29, -85320.29, -85320.29, -85320.29, -85320.29, -85320.29, -85320.29, -85320.29]
[66471.846, 66471.85, 66471.846, 66471.847, 66471.846, 66471.846, 66471.846, 66471.846]
[123456789.98765, 123456789.99, 123456789.987, 123456789.988, 123456789.987, 123456789.988, 123456789.988, 123456789.988]
[0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1, 0.1]
[0.0, 0.0, 0.0, 0.001, 0.0, 0.0, 0.0, 0.0]
[0.0, 0.0, -0.001, 0.0, 0.0, 0.0, 0.0, 0.0]
[1235, 1200, -1200, 1200, 1300, 1200]
[64.782, 64.78199, 64.782, 64.78199]
[1.0e+20, 0.0, 0.0, -0.0]

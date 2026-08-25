# `BigDecimal#round` with an explicit digits argument answers a
# BigDecimal (only the no-argument form answers Integer); zeo answers
# Integer for the digits form too. (Found by the 2026-08-24 probe
# sweep.)
require "bigdecimal"
p BigDecimal("2.5").round(0, half: :even)
p BigDecimal("2.5").round(0).class
p BigDecimal("2.5").round.class

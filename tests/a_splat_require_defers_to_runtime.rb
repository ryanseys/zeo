# `require(*names)` has no compile-time feature name -- the call stays live
# and the runtime `Kernel#require` raises the catchable `LoadError`, which
# is the whole point: every corpus hit sits inside a `rescue LoadError`
# optional-dependency helper (wagons, right_support).
#
# zeo's require recognizer checked the argument COUNT (one) before the
# shape, so a single splat slipped past it into the generic lowering and
# died on "unsupported syntax at \"*names\"".
def try_require(*names)
  require(*names)
  "loaded"
rescue LoadError
  "unavailable"
end

puts try_require("zeo_definitely_does_not_ship_this")

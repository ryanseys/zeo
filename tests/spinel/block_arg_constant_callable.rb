# `&SOME_LAMBDA` forwards a callable the same way `&some_lambda` does: a
# constant read is as deterministic and side-effect-free as a local one. Only
# locals and ivars were accepted, so the constant form was refused outright.
DOUBLE = ->(x) { x * 2 }
p [1, 2, 3].map(&DOUBLE)

TO_S = ->(x) { x.to_s }
p [1, 2].map(&TO_S)
p [1, 2, 3].select(&->(x) { x > 1 })

BIG = ->(x) { x > 2 }
p [1, 2, 3, 4].select(&BIG)
p [1, 2, 3, 4].reject(&BIG)
p [3, 1, 2].sort_by(&DOUBLE)
p [1, 2, 3].each(&DOUBLE)

module M
  F = ->(x) { x + 1 }
end
p [1, 2].map(&M::F)

# the local and ivar forms keep working
f = ->(x) { x * 3 }
p [1, 2].map(&f)

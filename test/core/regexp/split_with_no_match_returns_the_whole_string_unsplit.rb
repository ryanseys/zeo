# The empty-haystack-returns-an-empty-array case is exercised directly
# at the `zeo_rt::regexp_split` unit-test level instead (see
# `zeo-rt/src/regexp.rs`) -- `puts`-ing it here would conflate this
# phase's own behavior with a separate, pre-existing, unrelated gap:
# `Kernel#puts` on an EMPTY `Array` currently prints a blank line
# (real Ruby prints nothing at all for `puts []`) -- confirmed via a
# plain, regex-free `puts []; puts "x"` repro, so not something this
# phase introduces or should fix as a side effect. `Array#length`
# doesn't sidestep this cleanly either: `split`'s result has no static
# `TyKind` seeding (unlike a literal `[]`), so it stays `Poly` and hits
# the same "no static-array fast path" limitation.

puts "abc".split(/x/)
__END__
abc

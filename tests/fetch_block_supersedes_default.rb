# CRuby warns "block supersedes default value argument" wherever a `fetch`-like
# takes BOTH a positional default and a block, and the block is the one that
# wins. Six sites carry the same wording (`Hash#fetch`, `Array#fetch`,
# `Array.new`, `ENV.fetch`, `Thread#fetch`), and the warning is `rb_warn`, not
# `rb_warning`: it prints under a plain run, not only under `-w`.

# The block outranks the default at every site.
p([1, 2].fetch(9, :dflt) { |i| [:blk, i] })
p({ a: 1 }.fetch(:z, :dflt) { |k| [:blk, k] })
p(ENV.fetch("ZEO_NO_SUCH_VAR", "dflt") { |k| "blk #{k}" })
p(Thread.current.fetch(:nope, :dflt) { |k| [:blk, k] })
p(Array.new(3, :fill) { |i| i })

# A found key never reaches either, so no warning and no block call.
p([1, 2].fetch(0, :dflt) { :blk })
p({ a: 1 }.fetch(:a, :dflt) { :blk })

# One argument plus a block is the ordinary form -- no warning.
p([1, 2].fetch(9) { |i| [:blk, i] })
p({ a: 1 }.fetch(:z) { |k| [:blk, k] })
p(ENV.fetch("ZEO_NO_SUCH_VAR") { |k| "blk #{k}" })
p(Thread.current.fetch(:nope) { |k| [:blk, k] })
p(Array.new(3) { |i| i * 2 })

# Two arguments and no block is the other ordinary form -- no warning.
p([1, 2].fetch(9, :dflt))
p({ a: 1 }.fetch(:z, :dflt))
p(ENV.fetch("ZEO_NO_SUCH_VAR", "dflt"))
p(Thread.current.fetch(:nope, :dflt))
p(Array.new(2, :fill))

# `$VERBOSE = nil` silences `rb_warn` entirely.
$VERBOSE = nil
p({ a: 1 }.fetch(:z, :dflt) { :blk })
$VERBOSE = false

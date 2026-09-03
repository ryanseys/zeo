# `alias_method :eql?, :==` in one file, `def ==` in another. The alias must
# bind that `==`, so two equal values are `eql?` and `Array#-` subtracts them.
#
# zeo bound `Kernel#eql?` -- identity -- because `resolve_aliases` compares the
# alias's `seq` against the def's, and the analyze walk numbers the whole main
# file before the first feature unit while a unit's body really runs at its
# `require`. Across two streams those numbers order nothing, so the def read as
# "defined later" and the alias fell through to the ancestors.
#
# bundler writes exactly this on `Gem::Dependency` (rubygems_ext.rb), and its
# `==` lives in rubygems' dependency.rb. Every locked gem then compared unequal
# to its own Gemfile twin, `@dependencies - @locked_deps.values` subtracted
# nothing, and `bundle install` refused all 69 as newly added.
#
# The companion rule is `an_alias_of_a_builtin_keeps_the_old_body.rb`: WITHIN
# one stream seq is real execution order, and the wrap idiom depends on it.

$LOAD_PATH.unshift File.expand_path(
  "an_alias_binds_a_body_from_another_stream/lib", __dir__
)
require "thing_ext"

a = Streamed::Thing.new(1)
b = Streamed::Thing.new(1)

m = a.method(:eql?)
p [m.owner.to_s, m.original_name.to_s]
p [a == b, a.eql?(b), a.hash == b.hash]
p [([a] - [b]).size, [a, b].uniq.size]
__END__
["Streamed::Thing", "=="]
[true, true, true]
[0, 1]

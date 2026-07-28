# Kernel#pp now compiles but dies at load time on pp.rb's `class << ENV`:
# ENV is a hand-rolled singleton object in zeo, not an ordinary one, so
# opening its singleton class raises TypeError ("can't define singleton
# method for this value").
#
# (This replaced a codegen type error, now fixed: a destructured block
# parameter captured by a nested block was cell-wrapped twice, once by the
# destructuring itself and again by the nested-capture prologue.)
require "pp"
pp({a: 1})

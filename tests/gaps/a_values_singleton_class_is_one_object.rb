# `v.singleton_class` must answer the SAME object every time. For a value
# receiver -- a String, an Integer-like, anything not an `Object`-backed
# instance -- zeo mints a fresh one per call, so `equal?` is false and any
# state written on it is lost.
#
# An `Object.new` receiver is already right, which is what narrows this to the
# value side. `value_ivars` and the singleton-owner table key on the value's
# ADDRESS (see `an_ivar_on_a_bare_value_survives_address_reuse.rb`); the
# singleton CLASS itself has no such table, so nothing holds the one that was
# minted.
#
# Found by the sweep for `a_singleton_body_ivar_reaches_the_singleton_class`,
# where `class << some_string; @tag = "x"; end` then read back nil. That
# rewrite is correct -- it lands on `some_string.singleton_class`, which is
# simply a different object by the time the read runs.

h = "str"
h.singleton_class.instance_variable_set(:@t, 1)
p h.singleton_class.instance_variable_get(:@t)
p h.singleton_class.equal?(h.singleton_class)

class << h
  @tag = "on the string's singleton"
end
p h.singleton_class.instance_variable_get(:@tag)

# The `Object` receiver that already works, as the control.
o = Object.new
o.singleton_class.instance_variable_set(:@t, 2)
p o.singleton_class.instance_variable_get(:@t)
p o.singleton_class.equal?(o.singleton_class)

# `v.singleton_class` must answer the SAME object every time. For a value
# receiver -- a String, an Array, a Hash, a Range, anything not an
# `Object`-backed instance -- zeo minted a fresh one per call, so `equal?` was
# false and any state written on it was lost.
#
# An `Object.new` receiver was already right, which is what narrowed it to the
# value side. The cache was there; its KEY was not. `singleton_class_key`
# answered `None` for everything but an Object and a Class, where the sibling
# `extend_key` -- the same question about the same receiver -- already keyed
# every heap value by its address. They are one function now, and
# `value_identity` covers every `Arc`-backed variant `weak_owner` can pin:
# Range, Proc, MatchData, Fiber, Enumerator, Thread, Mutex, Queue and Ractor
# joined the five that were there.
#
# The address is safe as a key for the same reason `value_ivars`' is: the row
# holds its owner, so the allocation cannot be freed and the address cannot be
# reused while the row lives (`an_ivar_on_a_bare_value_survives_address_reuse.rb`).
#
# Found by the sweep for `a_singleton_body_ivar_reaches_the_singleton_class`,
# where `class << some_string; @tag = "x"; end` then read back nil.
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

# Every heap kind answers one object, and defining on it works.
[ "s", [1], {a: 1}, (1..2), /x/, proc { 1 }, Thread::Mutex.new, Thread::Queue.new,
  /a/.match("a"), Object.new ].each do |v|
  puts "#{v.class}: #{v.singleton_class.equal?(v.singleton_class)}"
end

# A dup is a different object, so it gets a different singleton.
d = "dup me"
p d.dup.singleton_class.equal?(d.singleton_class)

# The state written on one is there through the next ask.
arr = [1]
arr.singleton_class.instance_variable_set(:@k, :v)
p arr.singleton_class.instance_variable_get(:@k)
def arr.second = self[1]
p arr.singleton_methods
p arr.singleton_class.instance_methods(false)

# An `extend` files its module in the singleton's ancestry.
module Tag; def tagged = :yes; end
str = "x"
str.extend(Tag)
p [str.tagged, str.is_a?(Tag), str.singleton_class.include?(Tag)]

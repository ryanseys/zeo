# Two divergences on the same storage, both on a BARE `Object` and neither on a
# user class, which is why the corpus never saw them.
#
# `Object.new`'s ivars were a plain `HashMap`, so `instance_variables` and
# `inspect` reported them in hash order rather than FIRST-ASSIGNMENT order.
# The store's own doc called that "limited to the name-keyed main object";
# the emitter folds every `Object.new` to the same type, so it was every one
# of them. `DynObject` -- an instance of a class minted at run time -- already
# used an insertion-ordered map, and says why.
#
# `Marshal.load` allocated through `dispatch::allocate_of`, which is documented
# to answer `None` for a BUILTIN because a builtin's blank instance lives with
# `Class#allocate`. `Object` is a builtin, so a dumped plain object could not
# be loaded back at all: `TypeError: allocator undefined for Object`.
o = Object.new
o.instance_variable_set(:@b, "two")
o.instance_variable_set(:@a, [1, 2])
r = Marshal.load(Marshal.dump(o))
p r.class, r.instance_variables, r.instance_variable_get(:@a), r.instance_variable_get(:@b)
p r.equal?(o)

n = Object.new
n.instance_variable_set(:@inner, o)
p Marshal.load(Marshal.dump(n)).instance_variable_get(:@inner).instance_variable_get(:@b)

# The order is the OBJECT's, not the class's, and every route reports the same
# one -- including a removal, which leaves no hole for a later name to fill.
q = Object.new
q.instance_variable_set(:@z, 1)
q.instance_variable_set(:@m, 2)
q.instance_variable_set(:@a, 3)
p q.instance_variables
p q.inspect.sub(/0x[0-9a-f]+/, "0xADDR")
p q.dup.instance_variables
p q.clone.instance_variables
q.remove_instance_variable(:@m)
p q.instance_variables
q.instance_variable_set(:@m, 9)
p q.instance_variables
p Marshal.load(Marshal.dump(q)).instance_variables

f = Object.new
f.instance_variable_set(:@x, 1)
f.freeze
p f.frozen?, f.dup.frozen?, f.clone.frozen?
p Marshal.load(Marshal.dump(f)).frozen?
__END__
Object
[:@b, :@a]
[1, 2]
"two"
false
"two"
[:@z, :@m, :@a]
"#<Object:0xADDR @z=1, @m=2, @a=3>"
[:@z, :@m, :@a]
[:@z, :@m, :@a]
[:@z, :@a]
[:@z, :@a, :@m]
[:@z, :@a, :@m]
true
false
true
false

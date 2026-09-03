# `obj.singleton_methods` finds a `def obj.x`, but the singleton CLASS does not
# report it: `instance_methods(false)` is empty, `method_defined?` is false and
# `instance_method` raises `NameError`.
#
# A per-object singleton lives in `runtime_meta`'s identity-keyed `singletons`
# table, and `singleton_methods` reads it by identity. `singleton_class` mints a
# class id and records the owner both ways (`singleton_classes`/
# `singleton_owner`), but Module's reflection rows answer from the CLASS-keyed
# tables, which that id has no rows in -- so the two views of the same method
# disagree.
#
# A CLASS's singleton (`def K.cm`) already reports correctly, because a class
# method is stored class-keyed to begin with. Only the per-object case splits.

o = Object.new
def o.hi = 1

p o.singleton_methods
p o.singleton_class.instance_methods(false)
p o.singleton_class.public_instance_methods(false)
p o.singleton_class.method_defined?(:hi)
p o.singleton_class.instance_method(:hi).owner == o.singleton_class

class K; end
def K.cm = 2
p K.singleton_class.instance_methods(false)
p K.singleton_methods
__END__
[:hi]
[:hi]
[:hi]
true
true
[:cm]
[:cm]

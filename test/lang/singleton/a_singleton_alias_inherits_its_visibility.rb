# An `alias_method` on an object's SINGLETON class installs a public row even
# when its source is private. Ruby gives an alias the visibility of the method
# it copies, on a singleton exactly as on a class.
#
# The class-side rule is already right: `alias_method :y, :x` after
# `private :x` makes `y` private, and the `class << self` twin does too. The
# per-object path writes the body into the identity-keyed singleton table and
# nothing carries the source's mark across with it.

def try(label)
  p [label, yield]
rescue NoMethodError, NameError => e
  p [label, :raised, e.class.to_s]
end

o = Object.new
def o.a = :a
o.singleton_class.send(:private, :a)
o.singleton_class.send(:alias_method, :b, :a)
try(:alias_public) { o.public_send(:b) }
try(:alias_send)   { o.send(:b) }
try(:singles)      { o.singleton_methods.sort }
try(:respond)      { o.respond_to?(:b) }

# A PUBLIC source aliases public.
q = Object.new
def q.c = :c
q.singleton_class.send(:alias_method, :d, :c)
try(:pub_alias)    { q.public_send(:d) }
try(:pub_singles)  { q.singleton_methods.sort }

# The `class << obj; alias new old; end` spelling takes the same route.
r = Object.new
def r.e = :e
r.singleton_class.send(:private, :e)
class << r
  alias f e
end
try(:kw_alias)     { r.public_send(:f) }
try(:kw_send)      { r.send(:f) }
__END__
[:alias_public, :raised, "NoMethodError"]
[:alias_send, :a]
[:singles, []]
[:respond, false]
[:pub_alias, :c]
[:pub_singles, [:c, :d]]
[:kw_alias, :raised, "NoMethodError"]
[:kw_send, :e]

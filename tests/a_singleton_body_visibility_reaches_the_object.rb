# A compiled `class << obj` body's visibility directives never reach the
# object, so every method it writes installs PUBLIC.
#
# The body runs against a compiled SURROGATE class id. A bare `private` there
# has no runtime body frame to record into, so `runtime_set_visibility` parks
# it as the surrogate's `singleton_default_vis` (`runtime_meta/api.rs:998`),
# and `runtime_define_method`'s singleton redirect reads it back. That redirect
# then applies the mark ONLY when the owner is a `Class` -- it calls
# `runtime_class_method_visibility`, and an ordinary object has no class-method
# table. For an object owner the cursor is dropped on the floor.
#
# `private :name` and `private def` in the same body take the other half of
# `runtime_set_visibility`, which writes `methods_vis` on the SURROGATE id --
# a table no reader of this object's rows ever consults, because per-object
# singleton rows are keyed by identity.
#
# The three spellings are one cause and are pinned together here.

def try(label)
  p [label, yield]
rescue NoMethodError, NameError => e
  p [label, :raised, e.class.to_s]
end

o = Object.new
class << o
  def pub = :pub
  private
  def cursor = :cursor
  protected
  def prot = :prot
end
try(:cursor_public) { o.public_send(:cursor) }
try(:cursor_send)   { o.send(:cursor) }
try(:prot_public)   { o.public_send(:prot) }
try(:pub_public)    { o.public_send(:pub) }
try(:singletons)    { o.singleton_methods.sort }
try(:privates)      { o.private_methods(false).include?(:cursor) }
try(:protecteds)    { o.protected_methods(false) }

o2 = Object.new
class << o2
  private def inline = :inline
  def open = :open
end
try(:inline_public) { o2.public_send(:inline) }
try(:inline_list)   { o2.singleton_methods.sort }

o3 = Object.new
class << o3
  def named = :named
  private :named
end
try(:named_public)  { o3.public_send(:named) }
try(:named_list)    { o3.singleton_methods }

# A fresh `def` resets the name to the running default, on an object exactly
# as on a class.
o4 = Object.new
class << o4
  private
  def reset = :first
end
def o4.reset = :second
try(:reset_public)  { o4.public_send(:reset) }
try(:reset_list)    { o4.singleton_methods }

# `clone` carries the singleton table, so it carries the marks with it.
o5 = Object.new
class << o5
  private
  def carried = :carried
end
try(:clone_public)  { o5.clone.public_send(:carried) }
try(:clone_list)    { o5.clone.singleton_methods }
try(:dup_list)      { o5.dup.singleton_methods }

# A non-object heap value keeps its rows in a table of its own, and the mark
# has to follow them there.
s = +"str"
class << s
  private
  def shout = :shout
end
try(:value_public)  { s.public_send(:shout) }
try(:value_list)    { s.singleton_methods }

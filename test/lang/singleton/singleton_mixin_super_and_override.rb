# Two limits of zeo's per-object singleton, both reachable through plain
# `obj.extend(M)` and therefore PRE-EXISTING -- routing `class << obj; include
# M` to the same machinery does not create them, it just gives them a second
# spelling.
#
# zeo implements a singleton as a COPIED method table
# (`runtime_meta::extend_object_default`), where CRuby splices the module into
# the singleton class's ancestry. Copying loses the module's POSITION, so:
#
#   1. `super` from a mixed-in method has nowhere to resume -- it should reach
#      the receiver class's own definition.
#   2. a mixed-in method does not reliably outrank the class's own for a
#      statically-bound call site, because the compiled site resolved before
#      the extend ran.
#
# Fix shape: the singleton needs real spliced ancestry the owner's dispatch
# walks, not a copied table -- which is also what would let `include` and
# `prepend` differ from each other, as CRuby's do.

module Loud
  def speak
    "LOUD(" + super + ")"
  end
end

class Speaker
  def speak
    "plain"
  end
end

# (1) `super` from an extended module.
a = Speaker.new
a.extend(Loud)
p a.speak

# (2) the extended module outranking the class's own method.
b = Speaker.new
b.extend(Loud)
p b.speak

# The same two through the `class << obj` spelling.
c = Speaker.new
class << c
  prepend Loud
end
p c.speak

# An extend that ADDS a name (rather than overriding one) already works, and is
# what the corpus actually needs.
module Extra
  def extra
    "extra"
  end
end

d = Speaker.new
d.extend(Extra)
p [d.extra, d.speak, Speaker.new.respond_to?(:extra)]
__END__
"LOUD(plain)"
"LOUD(plain)"
"LOUD(plain)"
["extra", "plain", false]

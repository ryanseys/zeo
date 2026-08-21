# `#dup` drops the singleton methods, so the copy is an instance of the class
# Ruby sees. The copy is made by assigning the whole struct, which carried the
# ORIGINAL's cls_id -- for a singleton receiver that is the synthesized
# subclass -- so the copy answered the singleton's methods at run time while
# `respond_to?`, reading the static type, said it did not (#4043).
class Sing
  def base = 1
end

o = Sing.new
def o.extra = 99

p o.extra
p((o.dup.extra rescue $!.class))
p o.dup.respond_to?(:extra)
p o.dup.base
p o.dup.class
p o.respond_to?(:extra)

# clone KEEPS them, as CRuby does
p o.clone.extra
p o.clone.respond_to?(:extra)

# a module extended onto the object goes the same way
module Loud
  def shout = "LOUD"
end
q = Sing.new
q.extend(Loud)
p q.shout
p((q.dup.shout rescue $!.class))
p q.clone.shout

# a plain object with no singleton is unaffected
r = Sing.new
p r.dup.base
p r.dup.class

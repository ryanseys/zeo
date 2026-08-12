# `obj.extend(M)` seats M in THAT OBJECT's singleton ancestry, so the method
# it supplies is owned by M -- the same rule that now holds for a class-level
# `extend` (see an_extended_modules_row_answers_as_the_modules_own.rb). zeo
# reports the object's CLASS instead.
#
# WHY. A class records the copies an `extend` made in
# `OverlayEntry::extended_class_methods`, so reflection can tell a copied row
# from a `def self.x` written on the class. An OBJECT has no such record: both
# `obj.extend(M)` and `def obj.hi` write straight into the identity-keyed
# `maps().singletons` table, and nothing afterwards can say which put a name
# there. Guessing "an extended module that defines the name owns it" is wrong
# exactly when a later `def obj.hi` shadows one, so the fix is the record, not
# the guess: an extended-name set beside the singleton table, cleared by every
# own definition, mirroring what the class side already keeps.
#
# Everything else about a per-object extend already agrees, and is asserted in
# the golden beside this one.
module Ext
  def hi = 1
end

o = Object.new
o.extend(Ext)
p o.method(:hi).owner
p o.method(:hi).unbind.owner
p o.singleton_method(:hi).owner rescue p $!.class

# A per-object `def` written after the extend must keep reporting the
# singleton class, which is what makes the guess unusable.
def o.hi = 2
p o.method(:hi).owner
p o.hi

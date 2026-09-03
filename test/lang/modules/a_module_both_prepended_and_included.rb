# A module PREPENDED and INCLUDED on the same class appears TWICE in CRuby's
# ancestry -- once ahead of the class, once behind it -- and its body runs
# once per position.
#
# THE EXACT CRUBY RULE, measured against the oracle. It is asymmetric, and the
# asymmetry is the whole of the behaviour:
#
#   include A; prepend A   ->  [A, K, A]   the module appears TWICE
#   prepend A; include A   ->  [A, K]      the include is a no-op
#   include A; include A   ->  [K, A]
#   prepend A; prepend A   ->  [A, K]
#
# `rb_include_module` calls `include_modules_at(klass, ORIGIN(klass), module,
# search_super = TRUE)`, so `include` searches the WHOLE chain and skips a
# module already anywhere in it. `rb_prepend_module` passes
# `search_super = FALSE`, so `prepend` searches only the prepend area and adds
# a module that is merely included below. DOCUMENT ORDER therefore decides:
# whichever verb runs first is the one that finds an empty chain.
#
# The duplicate is what makes `super` interesting. zeo's ancestry is a flat
# `Vec<ClassId>` and every walk positions by the FIRST match, so a `super`
# written in the doubled module resumed past the first copy forever and
# re-found itself. The baked `defining_class` a compiled body hands `super`
# names the module, which cannot say which copy is running; CRuby has the
# answer on the control frame (`cfp->cme->defined_class` is the ICLASS).
# `dispatch::MRO_RESUME` is that fact, published by the walk that enters the
# body and gated on a chain anywhere in the process holding a repeat.
#
# `Method#super_method` needs the same occurrence, or the re-seat finds the
# first copy again and the chain never ends.

module MX
  def hi = "mx(#{defined?(super) ? super : 'top'})"
end

class Both
  include MX
  prepend MX
  def hi = "Both"
end

p Both.ancestors.map(&:to_s)
p Both.new.hi

class Runtime
  def hi = "Runtime"
end
Runtime.include MX
Runtime.prepend MX
p Runtime.ancestors.map(&:to_s)

# `super` runs the doubled module ONCE PER POSITION, with the class's own
# body between the two.
$ran = 0
module MW
  def go = ($ran += 1; "mw(#{defined?(super) ? super : 'top'})")
end

class Sandwich
  include MW
  prepend MW
  def go = "own(#{defined?(super) ? super : 'top'})"
end

p Sandwich.ancestors.map(&:to_s)
p Sandwich.new.go
p $ran

# A fresh dispatch from INSIDE the second copy starts over at the first.
$seen = []
module MV
  def hop(d)
    $seen << "mv(#{d})"
    Nested.new.hop(2) if d == 1
    defined?(super) ? super : "top"
  end
end
class Nested
  include MV
  prepend MV
end
p Nested.new.hop(1)
p $seen

# `Method`/`UnboundMethod` walk both copies and then stop.
def owners(m)
  out = []
  while m
    out << m.owner.to_s
    m = m.super_method
  end
  out
end
p owners(Sandwich.new.method(:go))
p owners(Sandwich.instance_method(:go))
__END__
["MX", "Both", "MX", "Object", "Kernel", "BasicObject"]
"mx(Both)"
["MX", "Runtime", "MX", "Object", "Kernel", "BasicObject"]
["MW", "Sandwich", "MW", "Object", "Kernel", "BasicObject"]
"mw(own(mw(top)))"
2
"top"
["mv(1)", "mv(2)", "mv(2)", "mv(1)", "mv(2)", "mv(2)"]
["MW", "Sandwich", "MW"]
["MW", "Sandwich", "MW"]

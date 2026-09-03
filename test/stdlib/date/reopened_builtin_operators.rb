# An operator redefined on a reopened builtin.
#
# zeo rejected this for EVERY builtin, on the reasoning that a native operator
# fast path at the call site would silently bypass the override. Only `Int` and
# `Float` have such a path: `codegen::call::dispatch` emits the four
# `ops::{INT,FLOAT}_{BINARY,UNARY}_OPS` arms and then the reopened-builtin arm,
# which sits deliberately ahead of the collection, Proc, Regexp and String fast
# paths so that a user redefinition wins there.
#
# The gems: activesupport reopens `DateTime#<=>`, fast_gettext `String#%`,
# strscan's own truffleruby shim `StringScanner#<<`, celluloid `Set#<<`.

require "set"
require "strscan"
require "date"

class String
  def %(other)
    "formatted(#{other})"
  end
end

class Set
  def <<(x)
    add(x.to_s)
  end
end

class StringScanner
  def <<(s)
    string + s
  end
end

class DateTime
  def <=>(_other)
    7
  end
end

# A statically-typed receiver, which is the arm the old guard was protecting.
p("x" % 1)
p(StringScanner.new("ab") << "cd")

s = Set.new
s << 3
p s.to_a

p(DateTime.new(2020, 1, 1) <=> DateTime.new(2021, 1, 1))

# A Poly receiver. zeo never infers a parameter's type from its call sites, so
# these reach the runtime-checked fallback -- which matches `Int`/`Float` and
# sends every other shape through `send_value_in`'s MRO walk, where the
# reopened row lives.
def apply(a, b)
  a % b
end

def push(coll, x)
  coll << x
end

p apply("y", 2)
p push(StringScanner.new("q"), "z")

t = Set.new
push(t, 5)
p t.to_a

# Integer and Float stay rejected, and so does anything else on their MRO
# (`Numeric`, `Comparable`, `Object`, `Kernel`, `BasicObject`) -- a statically
# `Int`-typed receiver consults all of them.
p 1 + 2
p 1.5 * 2
__END__
"formatted(1)"
"abcd"
["3"]
7
"formatted(2)"
"qz"
["5"]
3
3.0

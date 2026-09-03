# `def fetch(name, default = (no_default = true))` is the idiom for "was an
# argument actually passed?" -- and the local the default assigns belongs to the
# WHOLE method, not to the default expression. It is nil when the default did
# not run, and every block in the body can read it.
#
# Defaults are not part of a method's body, so a pass that collects locals by
# walking the body alone never sees the name. A block capturing it then
# fresh-declared its own and read `nil` -- silently, with no diagnostic -- and a
# NESTED block hit a refusal instead. memoizable's `Memory#fetch` is the shape,
# three block levels deep.

def fetch(name, default = (no_default = true))
  [1].each do
    [2].each do
      [3].each do
        p [name, no_default, default]
      end
    end
  end
end

fetch(:given)
fetch(:passed, 9)

# One level is the same question, and read `nil` before.
def one_level(d = (flag = true))
  [1].each { p [d, flag] }
end

one_level
one_level(:explicit)

# The local is a real method local outside any block, too.
def plainly(d = (seen = :yes))
  [d, seen]
end

p plainly
p plainly(:passed)

# A default may assign more than one, and a later default may READ an earlier
# parameter -- ruby evaluates them left to right.
def several(a = (x = 1), b = a + 1, c = (y = b * 2))
  [a, b, c, x, y]
end

p several
p several(10)
p several(10, 20)

# A keyword parameter's default scopes its local the same way.
def keyworded(k: (kw_flag = :defaulted))
  [1].each { p [k, kw_flag] }
end

keyworded
keyworded(k: :given)

# ... and the name survives being ASSIGNED again inside a block, which makes it
# a shared cell rather than a per-block copy.
def reassigned(d = (count = 0))
  [1, 2, 3].each { count = (count || 0) + 1 }
  [d, count]
end

p reassigned
p reassigned(:passed)

# When the default did NOT run, the name is simply nil -- it is declared for the
# whole method either way.
def untouched(d = (never = :ran))
  [d, never]
end

p untouched(:passed)

# A block that escapes (a Proc kept past the call) reads the same cell.
def escaping(d = (tag = :from_default))
  -> { [d, tag] }
end

p escaping.call
p escaping(:passed).call
__END__
[:given, true, true]
[:passed, nil, 9]
[true, true]
[:explicit, nil]
[:yes, :yes]
[:passed, nil]
[1, 2, 4, 1, 4]
[10, 11, 22, nil, 22]
[10, 20, 40, nil, 40]
[:defaulted, :defaulted]
[:given, nil]
[0, 3]
[:passed, 3]
[:passed, nil]
[:from_default, :from_default]
[:passed, nil]

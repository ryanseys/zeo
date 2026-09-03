# Expression shapes that were previously clean compile rejections:
# multi-value return/break/next, parenthesized statement sequences, the
# fuller interpolation forms, and top-level instance variables.

# --- return / break / next with several values ------------------------------
# More than one value builds an implicit array -- the same array the
# equivalent literal would, splats included.
def pair
  return 1, 2
end
p pair

def triple
  return 1, 2, 3
end
p triple

def with_splat(a)
  return 1, *a
end
p with_splat([2, 3])

# A SINGLE splat is an array too -- `return *a` where a == [1] is [1], not 1.
def splat_only(a)
  return *a
end
p splat_only([1, 2])
p splat_only([1])
p splat_only([])

# A bare `return` is still nil.
def nothing
  return
end
p nothing

# break/next take the same shape.
p([1].each { break 1, 2 })
p([[1, 2]].map { |a, b| next a, b })

# --- parenthesized statement sequences --------------------------------------
# `(a; b)` evaluates each in order and answers the LAST.
p((1; 2; 3))

# It introduces NO scope of its own -- a local assigned inside is still
# readable afterwards.
doubled = (n = 5; n * 2)
p doubled
p n

# Newline-separated is the same construct.
computed = (
  base = 7
  base + 1
)
p computed

# In argument and element position, and nested.
p((puts "side effect"; 42))
p [(1; 2), 3]
p((1; (2; 3)))

# A statement can be any expression, including an `if`.
p((if true then "yes" else "no" end; "after"))

# One statement in parens is just that expression.
p (5)

# --- string interpolation forms ---------------------------------------------
# Several statements inside #{ } answer the last...
p "v=#{1; 2}"
p "v=#{x = 3; x * 2}"
p x                       # ...and, like parens, leak their locals

# An empty #{} interpolates nothing.
p "a#{}b"

# Brace-less #@ivar / #$global interpolation.
$greeting = "hi"

class Speaker
  def initialize
    @name = "sam"
  end

  def to_s
    "#$greeting, #@name"
  end
end
puts Speaker.new

# --- top-level instance variables -------------------------------------------
# At the top level `self` is `main`, an ordinary object -- so `@x` there is
# just its instance variable. A top-level `def` is a private method OF that
# same object, so it sees the same storage.
@config = "set at top level"
p @config
p @never_assigned          # nil, not an error

def read_config
  @config
end
p read_config

def bump
  @count = (@count || 0) + 1
end
bump
bump
bump
p @count
__END__
[1, 2]
[1, 2, 3]
[1, 2, 3]
[1, 2]
[1]
[]
nil
[1, 2]
[[1, 2]]
3
10
5
8
side effect
42
[2, 3]
3
"after"
5
"v=2"
"v=6"
3
"ab"
hi, sam
"set at top level"
nil
"set at top level"
3

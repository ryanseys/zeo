# The smaller corners of Ruby's syntax: the last-match globals, BEGIN/END,
# undef, the alias forms, hash shorthand, interpolated symbols, and the
# places a splat can appear.
#
# `__FILE__`/`__LINE__`/`__dir__` are exercised in the test suite rather than
# here: their answers embed this file's own path, which differs per machine
# and so can't be diffed against a fixed expectation.

# --- BEGIN / END ------------------------------------------------------------
# BEGIN bodies run before ANY main statement, in source order; END bodies run
# at exit, in REVERSE order (END is `at_exit` exactly).
END { puts "end: written first, runs last" }
BEGIN { puts "begin: written second, runs first" }

# --- the last-match globals -------------------------------------------------
if "hello world" =~ /(\w+)\s(\w+)/
  p $1
  p $2
  p $3                    # a group that doesn't exist is nil
  p $&                    # the whole match
  p $`                    # before it
  p $'                    # after it
  p $~.class
end

# The match was at the start, so `$\`` is empty rather than nil -- nil is
# reserved for "there was no match at all".
"xyz" =~ /y/
p [$`, $&, $']

# A FAILED match CLEARS them all. This is what makes the `if ... then $1`
# shape safe to reuse: a stale previous match can never leak through.
"zzz" =~ /(\d+)/
p [$1, $&, $~]

# `match?` is the allocation-free predicate: it builds no MatchData, so it
# leaves the slot alone.
"abc" =~ /b/
"xyz".match?(/y/)
p $&                      # still "b"

# `Regexp#match` does set it.
/(\d)/.match("a1")
p $1

# `$~` is a real MatchData.
"hello" =~ /e(l+)(o)/
m = $~
p m[0]
p m[1]
p m.captures
p m.pre_match
p m.post_match

# --- named-capture auto-binding ---------------------------------------------
# `/(?<name>..)/ =~ str` assigns each named group to a LOCAL of that name.
if /(?<first>\w+) (?<last>\w+)/ =~ "John Smith"
  puts first
  puts last
end

# It still answers what `=~` answers, so it works as a condition.
p(/(?<n>\d+)/ =~ "abc123")
p n

# A failed match leaves each name nil.
/(?<z>\d+)/ =~ "none"
p z

# Only with the literal on the LEFT: `str =~ /(?<a>.)/` binds nothing. That
# asymmetry is Ruby's own -- the parser can only declare the locals when it
# can see the names.

# --- undef ------------------------------------------------------------------
class Base
  def inherited_m = "from Base"
  def kept = "kept"
end

class Child < Base
  def own_m = "own"
  undef own_m
  undef inherited_m       # works on a name this class only INHERITS
end

begin
  Child.new.own_m
rescue NoMethodError
  puts "own_m: NoMethodError"
end

begin
  Child.new.inherited_m
rescue NoMethodError
  puts "inherited_m: NoMethodError"
end

p Base.new.inherited_m    # the ancestor is untouched
p Child.new.kept          # other inherited methods still work
p Child.new.respond_to?(:inherited_m)

# Several at once.
class Multi
  def a = 1
  def b = 2
  undef a, b
end
p Multi.new.respond_to?(:a)

# --- alias ------------------------------------------------------------------
# On a method: a second name for the same body. (The `alias_method` spelling
# is not exercised here -- it is a separate, still-open gap.)
class Greeter
  def hello = "hi"
  alias greet hello
end
p [Greeter.new.hello, Greeter.new.greet]

# On a GLOBAL: a real, bidirectional alias -- one slot, two names. Writing
# either is visible through the other, which is why it isn't a copy.
$orig = 5
alias $copy $orig
p $copy
$copy = 7
p $orig                   # written through the alias
$orig = 9
p [$orig, $copy]          # ...and through the original

# Aliasing a never-set global is legal; both read nil until one is written.
alias $late $not_yet_set
p $late
$not_yet_set = "now"
p $late

# --- hash shorthand ---------------------------------------------------------
# `{x:}` means `{x: x}`.
x = 1
name = "shorthand"
p({x:, name:})

# It works for keyword arguments too.
def describe(a:, b:)
  "#{a}/#{b}"
end
a = "left"
b = "right"
puts describe(a:, b:)

# --- interpolated symbols ---------------------------------------------------
who = "world"
p :"hello_#{who}"
p :"sum_#{1 + 1}"
p :"plain"
p :"a#{1}b".class

# --- splat positions --------------------------------------------------------
# In a `when`: every element is tested.
small = [1, 2]
case 1
when *small then puts "when-splat: hit"
else puts "when-splat: miss"
end

case 9
when *small then puts "when-splat: hit"
else puts "when-splat: miss"
end

# Mixed with listed values, and over a list of classes (`===` is is_a?).
case 5
when 3, *small, 5 then puts "when-splat: mixed hit"
end

kinds = [Integer, String]
case "s"
when *kinds then puts "when-splat: class hit"
end

# An empty splat matches nothing.
case 1
when *[] then puts "never"
else puts "when-splat: empty ok"
end

# Splatting a NON-array follows Ruby's `to_a` rules.
p [*[1, 2]]               # an Array is itself
p [*nil]                  # nil contributes NOTHING
p [*1]                    # no to_a: wrapped
p [*(1..3)]               # Range#to_a
p [*{a: 1}]               # Hash#to_a
p [*"str"]                # NOT chars -- String has no to_a

class HasToA
  def to_a = [7, 8]
end
p [*HasToA.new]           # a user to_a is honored

# The same rules drive a splatted multi-assignment RHS.
g, h = *1
p [g, h]
i, j = *[1, 2]
p [i, j]
k, l = *nil
p [k, l]

# In a `return`, and as a call argument.
def splat_return(a)
  return *a
end
p splat_return([1, 2])
p splat_return([1])       # a single splat is still an ARRAY

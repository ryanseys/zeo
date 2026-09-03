# Reopening a Rust builtin with a Ruby `initialize` runs that body.
#
# Ruby's `new` is `allocate` plus `initialize`, and a builtin's registered
# constructor FUSES the two, so the Ruby body was never consulted:
# `Pathname.new` left `@path` nil and all 94 of its methods read it. That is
# what kept the `pathname` corelib segment off.
#
# `Class#new` now asks whether a NON-NATIVE `initialize` wins the lookup. The
# two rows live in different tables -- a compile-time reopen registers on the
# VALUE channel, the native row is in the class's static method table -- so
# the answer is exact. The gap file said `method_owner` could not tell them
# apart; true, and it names the wrong table.
#
# Both reopen channels are covered below: a `class Pathname; def initialize`
# body, and a runtime `define_method(:initialize)`.
#
# NOT probed here, and not a bug this can fix: once a Ruby `initialize`
# replaces the native one and does not set the NATIVE state, a native reader
# sees that state's default where ruby sees nil (`Pathname#to_s` answers ""
# against ruby's nil). That is the Rust/Ruby hybrid seam, and it closes when
# the `pathname` corelib segment replaces the Rust rows outright. A reopen
# that adds only ordinary methods keeps the native constructor and is what
# every other Pathname test in the corpus exercises.
class Pathname
  def initialize(a) = @p = "P:#{a}"
  def mine = @p
end
p [Pathname.new("/x").mine, Pathname.new("/x").instance_variables]

# `Pathname.allocate` used to raise `allocator undefined`. A class states its
# own blank value with `allocate <fn>;` in its `ruby_class!` header.
p Pathname.allocate.inspect
p Pathname.allocate.instance_variables

# A runtime install reaches it through the overlay.
Pathname.class_eval do
  define_method(:initialize) { |a| @q = "Q:#{a}" }
  define_method(:other) { @q }
end
p [Pathname.new("/z").other, Pathname.new("/z").instance_variables]

# Everything else about a reopen already worked, and stays so a fix cannot
# regress it: an ordinary override, and a Ruby ivar on each Rust-backed value.
class Array
  def mine = "A-#{size}"
  def stash = (@a = 1)
end
p [[1, 2].mine, [1].stash]
class String
  def mine = "S"
  def stash = (@s = 7)
end
p ["q".mine, "q".stash]
class Hash
  def stash = (@h = 9)
end
p({}.stash)
class Exception
  def stash = (@e = 4)
end
p RuntimeError.new("x").stash
class Object
  def stash = (@o = 5)
end
p Object.new.stash

# A bootstrap builtin must NOT read as a reopen: exceptions keep their own
# rows on the object channel, and asking it lost every `RuntimeError.new`.
p RuntimeError.new("boom").message
p String.new("s")
p Array.new(2, 0)
p Hash.new(9)[:k]
__END__
["P:/x", [:@p]]
"#<Pathname:>"
[]
["Q:/z", [:@q]]
["A-2", 1]
["S", 7]
9
4
5
"boom"
"s"
[0, 0]
9

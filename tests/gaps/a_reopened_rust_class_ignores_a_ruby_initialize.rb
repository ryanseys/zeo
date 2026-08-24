# Reopening a Rust builtin class works for everything EXCEPT `initialize`:
# `Klass.new` runs the Rust constructor and the Ruby body never runs.
#
# The sweep below is the useful half of this file. Ivars are fine on every
# Rust-backed value -- String, Array, Hash, a Struct subclass, Exception,
# Object all store and read a Ruby `@x` written by a reopened method, because
# `value_ivars` keys them by address. So does an ordinary method override.
# What fails is one row, on the two classes whose constructor carries state:
#
#   class Pathname; def initialize(a) = @p = a; end
#   Pathname.new("/x").instance_variables   # ruby: [:@p]   zeo: []
#
# The cause is `builtins/rclass.rs`'s `Class#new`. Ruby's `new` is `allocate`
# plus `initialize`, and a builtin's registered constructor FUSES the two --
# `constructor_of(cid)` is called unconditionally, so an `initialize` the
# program defined is never consulted. `method_owner` cannot answer the
# question either: `Pathname` already owns a native `initialize`, so the Ruby
# row and the Rust row are indistinguishable through it.
#
# The fix is to unfuse: when the resolved `initialize` is a USER row rather
# than the class's own native one, `new` allocates through the registered
# allocator and dispatches `initialize` at the instance. That needs a
# predicate the dispatch tiers do not expose yet -- "the row that wins is not
# the native one" -- which is the same question
# `a_later_def_on_a_builtin_reaches_back` asks from the other side.
#
# This blocks the `pathname` corelib segment (docs/CORELIB.md): CRuby's own
# `pathname_builtin.rb` defines `initialize` and stores `@path`, so every one
# of its 94 methods reads nil until this is fixed.
class Pathname
  def initialize(a) = @p = "P:#{a}"
  def mine = @p
end
p [Pathname.new("/x").mine, Pathname.new("/x").instance_variables]

class Time
  def initialize(*) = @t = "T"
  def mine = @t
end
p [Time.new.mine, Time.new.instance_variables]

# Everything else about a reopen already works, and stays here so a fix
# cannot regress it.
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

# Issue #303: a module ivar initialized to an empty hash literal
# (`@slots = {}`) used to freeze its inferred type as `str_int_hash`
# (the empty-hash default) even when the module's class methods
# wrote a sym → string mapping into it. Result: the storage decl
# said `sp_StrIntHash *` but every `set` / `get` site emitted
# typed-mismatched calls (sym key into string-key hash, string
# value into int-value hash).
#
# Fix: a refinement pass in generate_code (after param inference
# stabilizes) walks each module's class-method bodies for
# `@<iname>[k] = v` writes and picks the most specific hash shape
# from the observed key + value types. Same idea for arrays via
# `@<iname> << v` / `@<iname>.push(v)`.

module M
  @slots = {}

  def self.set(k, v)
    @slots[k] = v
    nil
  end

  def self.get(k)
    @slots[k] || ""
  end

  def self.size
    @slots.length
  end
end

M.set(:foo, "bar")
M.set(:baz, "qux")
puts M.get(:foo)         # bar
puts M.get(:baz)         # qux
puts M.get(:missing)     # (empty default)
puts M.size              # 2

# Empty array literal — same refinement applies via `<<` writes.
module N
  @items = []

  def self.add(s)
    @items << s
  end

  def self.read(i)
    @items[i]
  end

  def self.size
    @items.length
  end
end

N.add("alpha")
N.add("beta")
N.add("gamma")
puts N.size              # 3
puts N.read(0)           # alpha
puts N.read(2)           # gamma

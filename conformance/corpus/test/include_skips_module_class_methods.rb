# Issue #714. `class C; include M; end` should NOT drag M's
# `def self.X` (class methods on the module) into C as instance
# methods. Doing so co-opted M's @ivar writes into C's ivar table and
# clobbered the hoisted `cst_M_<v>` slot, flipping its type from the
# correct sym_str_hash back to int and breaking subsequent hash ops.

module M
  @slots = {}
  def self.set(k, v); @slots[k] = v; nil; end
  def self.get(k); @slots.fetch(k, nil); end
end

class MyTest
  include M
  def run
    M.set(:title, "Hello")
    puts M.get(:title).inspect
  end
end

MyTest.new.run

# `MyTest.new` should NOT respond to `set` / `get` -- those are
# class methods on M, not instance methods. Probing via respond_to?
# (true/false) confirms the include didn't import them.
puts MyTest.new.respond_to?(:set)
puts MyTest.new.respond_to?(:get)

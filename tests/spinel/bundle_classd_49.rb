# Bundled tests:
#   - symbol_ivar_reassign
#   - unbox_poly_index_in_dispatch
#   - uncalled_method_dce
#   - undef
#   - uninit_attr_accessor_reads_as_nil

# === symbol_ivar_reassign ===
# Reassigning an instance variable to a different symbol used to demote
# its type to `poly` because `infer_ivar_init_type` reported `:foo` as
# `string` while `infer_type` reported it as `symbol`. The mismatch
# tripped `update_ivar_type`'s "old != new_type" branch, widening the
# field to `sp_RbVal`. Subsequent assignments emitted `self->s = SPS_bar`
# (sp_sym = mrb_int) into an `sp_RbVal` slot — a T_symbol_ivar_reassign_C type error.

class T_symbol_ivar_reassign_C
  def initialize
    @s = :foo
  end

  def reset
    @s = :bar
  end

  def show
    @s
  end
end

c = T_symbol_ivar_reassign_C.new
puts c.show
c.reset
puts c.show

# === unbox_poly_index_in_dispatch ===
# `emit_poly_builtin_dispatch` referenced `arg_types` out of scope.
# Without the fix, `(poly_recv)[poly_idx]` produced
# `sp_PolyArray_get(arr, sp_RbVal_idx)` — passing sp_RbVal where
# mrb_int is expected and failing the T_unbox_poly_index_in_dispatch_C compile.
#
# Trigger: poly recv (from heterogeneous Hash) + poly idx (also).

class T_unbox_poly_index_in_dispatch_C
  def initialize
    @bag  = { "arr" => [100, 200, 300, 400], "lbl" => "x" }
    @keys = { "i" => 2, "s" => "lbl" }
  end
  def at(k)
    arr = @bag["arr"]      # poly
    idx = @keys[k]         # poly
    arr[idx]               # poly recv + poly idx
  end
end

puts T_unbox_poly_index_in_dispatch_C.new.at("i").to_s    # use .to_s (handled separately) instead of .to_i

# === uncalled_method_dce ===
# Issue #393. An uncalled `def f(x); @typed = x; end` whose param
# defaulted to `mrb_int` (no caller pinned the type) tripped a T_uncalled_method_dce_C
# type mismatch against a narrower ivar slot like `const char *`.
#
# Fix: instance methods not reachable from any call site / SymbolNode
# / `super` get a stub body (`(void)params; return default;`).
# `initialize` and operator / conversion methods (`<=>`, `[]`, `to_s`,
# etc.) are always live regardless.

class T_uncalled_method_dce_C
  def initialize
    @body = ""
  end

  # Never called. Param defaults to mrb_int; @body is const char *.
  # Pre-fix: emit assigns `self->iv_body = lv_html` -- type mismatch.
  # Post-fix: body becomes `(void)lv_html; return 0;`.
  def render(html)
    @body = html
  end
end

c = T_uncalled_method_dce_C.new
puts "ok"

# === undef ===
# UndefNode -- `undef foo` inside a class body.
#
# CRuby raises NoMethodError if the undef'd method is called. In
# Spinel's AOT model we cannot dispatch at runtime (methods are
# static T_undef_C functions resolved at compile time), so `undef foo`
# becomes a compile-time error: any call to `.foo` on an instance
# of this class fails to compile with a precise message.
#
# This test verifies that defining + calling another method on the
# same class still works after undef -- undef removes only the
# named method, not the whole class.

class T_undef_C
  def foo = "foo"
  def bar = "bar"
  undef foo
end

c = T_undef_C.new
puts c.bar     # bar

# Multi-name form: `undef foo, bar` exercises the names-array path in
# spinel_parse.c (PM_UNDEF_NODE emits `names` as A(...)). Defining a
# third method `baz` and undef'ing the other two verifies that exactly
# the named methods are removed.
class T_undef_D
  def foo = "foo"
  def bar = "bar"
  def baz = "baz"
  undef foo, bar
end

puts T_undef_D.new.baz # baz

# === uninit_attr_accessor_reads_as_nil ===
# `attr_accessor :foo` with no `initialize`-time assignment must
# read as nil before any write. Pre-fix spinel registered the
# slot as the "int" placeholder, so the unset read returned 0
# (the type's zero) and downstream `"[#{a.counter}]"` rendered
# `[0]` instead of MRI's `[]`. Issue #634 shape B.
#
# The widening fires only when (a) the ivar is exposed via
# attr_reader / attr_accessor, (b) no method body assigns it,
# and (c) no writer call site has been observed during the
# iterative inference loop. T_uninit_attr_accessor_reads_as_nil_A class whose `reset` method writes
# the slot (optcarrot's APU oscillators) keeps the typed slot;
# `b.counter = 0` on a sibling instance also keeps the typed
# slot, but the uninitialized `a.counter` read on a fresh
# `T_uninit_attr_accessor_reads_as_nil_A.new` still returns nil through the per-instance poly
# storage.

class T_uninit_attr_accessor_reads_as_nil_A
  attr_accessor :counter
end

a = T_uninit_attr_accessor_reads_as_nil_A.new
puts "[#{a.counter}]"

b = T_uninit_attr_accessor_reads_as_nil_A.new
b.counter = 0
puts "[#{b.counter}]"

# Shape with attr_reader-only (no writer at all).
class T_uninit_attr_accessor_reads_as_nil_Box
  attr_reader :tag
end

box = T_uninit_attr_accessor_reads_as_nil_Box.new
puts "tag=[#{box.tag}]"


# Bundled tests:
#   - ivar_writer_heterogeneity
#   - kernel_module_function_call
#   - known_constants
#   - kwarg_default_empty_hash_kind
#   - kwargs_into_default_hash_param

# === ivar_writer_heterogeneity ===
# Issue #247: when two writers in different methods of the same class
# disagree on the value type, spinel used to narrow the slot to
# whichever writer's update_ivar_type call ran last — and the loser's
# emit site silently miscompiled. Here writer 1 in initialize assigns
# an int (via SymIntHash#[]) and writer 2 in write_any assigns a
# string param; the slot must widen to poly so both store cleanly.

class T_ivar_writer_heterogeneity_C
  def initialize(h)
    @id = h[:id]
  end

  def id; @id; end

  def write_any(value)
    @id = value
  end
end

c = T_ivar_writer_heterogeneity_C.new({id: 42})
c.write_any("string")
puts c.id

# Ensure the int-writer side still works when we don't re-write to a
# different type — the slot stays as observed.
class T_ivar_writer_heterogeneity_D
  def initialize(h)
    @v = h[:v]
  end
  def v; @v; end
end

d = T_ivar_writer_heterogeneity_D.new({v: 7})
puts d.v

# === kernel_module_function_call ===
# Issue #428. `Kernel.<m>` cmeth-style calls (the disambiguator
# users reach for when a `def self.<m>` shadows a Kernel
# module-function) emitted `0` instead of dispatching. Bare
# `<m>(...)` worked but couldn't be used inside a method body
# that shadowed the name -- which is exactly when callers
# write `Kernel.<m>`.
#
# Fix: in compile_object_method_expr's recv_type == "class"
# arm, when the recv AST is the ConstantReadNode "Kernel",
# route through compile_no_recv_call_expr. The bare-dispatch
# path already handles every Kernel module-function the
# runtime ships (sleep / puts / print / rand / etc.), so the
# Kernel-prefixed form gets the same shape uniformly.

class T_kernel_module_function_call_Sched
  def self.nap(seconds)
    # Kernel-prefixed to disambiguate from a hypothetical
    # def self.sleep on the same class.
    Kernel.sleep(seconds)
    0
  end
end

T_kernel_module_function_call_Sched.nap(0)
puts "ok"

# === known_constants ===
# Issue #75: the codegen now rejects an unknown constant reference
# with a "uninitialized constant <Name>" error instead of either
# leaking the bare identifier into the C output (`p Foo`) or
# silently lowering to `0` (`p Foo.bar`). This regression test pins
# the legitimate constant references that look superficially like
# the failing cases but resolve to known names — the fix shouldn't
# reject any of them.

# User-defined class — `find_class_idx` resolves it.
class T_known_constants_Box
  def initialize(v); @v = v; end
  attr_reader :v
end

puts T_known_constants_Box.new(7).v          # 7

# User-defined constant.
N = 42
puts N                     # 42

# Built-in module-like receiver (Math).
puts Math.sqrt(16).to_i    # 4

# ARGV / STDOUT pass through.
puts ARGV.length           # 0

# === kwarg_default_empty_hash_kind ===
# #486. Class method with a kwarg `set_cookies: {}` whose
# param is body-widened (via other call sites passing a
# sym-keyed hash) to sp_SymStrHash *. A bare call site that
# omits the kwarg synthesized the default at the call site as
# the generic sp_StrIntHash_new(), tripping
# -Wincompatible-pointer-types because the callee expects
# sp_SymStrHash *. Fix: the kwarg-default emit at the class-
# method dispatch site routes through empty_hash_coerce so an
# empty `{}` literal default takes on the param's declared
# hash variant.

class T_kwarg_default_empty_hash_kind_W
  def self.write(io, status, set_cookies: {})
    n = 0
    set_cookies.each do |name, val|
      n = n + 1
    end
    n
  end
end

class T_kwarg_default_empty_hash_kind_Use
  def self.kick
    cookies = { flash_notice: "Hi" }
    T_kwarg_default_empty_hash_kind_W.write(1, 200, set_cookies: cookies)
  end
end

puts T_kwarg_default_empty_hash_kind_W.write(1, 404).to_s

# === kwargs_into_default_hash_param ===
# When a method's only positional has a default hash literal
# (`attrs = {}`), call sites that pass kwargs whose names DON'T
# match the param name should bundle the kwargs into that hash
# slot. CRuby auto-folds trailing unmatched kwargs into a hash
# positional; spinel previously dropped them silently because:
#  - The analyzer fixpoint left the param typed `str_int_hash`
#    (from the literal `{}` default) and never widened despite
#    the call site carrying mixed-type values.
#  - The codegen `compile_typed_call_args` / class-method dispatch
#    arm filled the unmatched slot with the literal default
#    (`sp_StrIntHash_new()` / `sp_StrPolyHash_new()`), so the
#    callee saw an empty hash and constructed all-default values.
#
# Surfaced by Sam Ruby's roundhouse `comment_test.rb`
# `test_belongs_to_article_association` (issue #572 follow-up):
# `Comment.create(article_id: article.id, ...)` against
# `def self.create(attrs = {})` had the kwargs dropped, so the
# created Comment's article_id stayed 0 and the assertion
# `article.id != comment.article_id` raised.

class T_kwargs_into_default_hash_param_Bag
  def self.create(attrs = {})
    new(attrs)
  end
  def initialize(attrs = {})
    @id = attrs[:id] || 0
    @name = attrs[:name] || "(none)"
    @count = attrs.length
  end
  attr_reader :id, :name, :count
end

# Class method form: `.create(kw: val, ...)`.
b1 = T_kwargs_into_default_hash_param_Bag.create(id: 7, name: "alpha")
puts b1.id     # 7
puts b1.name   # alpha
puts b1.count  # 2

# Instance constructor form: `.new(kw: val, ...)` with default
# hash param. Without the analyzer widening, sp_Bag_new(0)
# segfaulted on the NULL hash. Issue #530 sibling.
b2 = T_kwargs_into_default_hash_param_Bag.new(id: 42, name: "beta")
puts b2.id     # 42
puts b2.name   # beta
puts b2.count  # 2

# Empty call: defaults still kick in.
b3 = T_kwargs_into_default_hash_param_Bag.create
puts b3.id     # 0
puts b3.name   # (none)
puts b3.count  # 0

# Mixed kwarg types -- string + int -- exercise the str_poly_hash
# poly value path.
b4 = T_kwargs_into_default_hash_param_Bag.create(id: 99, name: "gamma")
puts b4.id     # 99
puts b4.name   # gamma


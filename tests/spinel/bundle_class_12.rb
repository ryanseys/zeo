# Bundled tests:
#   - hash_fetch_hash_default_narrowing
#   - hash_fetch_string_default_on_int_leaf
#   - heterogeneous_dispatch_int_return_narrow
#   - if_static_false_skips_dead_branch_compile
#   - imeth_name_collision_recv_aware

# === hash_fetch_hash_default_narrowing ===
# Sequel to hash_fetch_hash_default: when `params.fetch "k", {}`
# is followed by `is_a?(Hash)` narrowing into an if-expression
# whose else arm returns an empty hash, the receiving local was
# typed as the concrete hash variant (str_int_hash) by the
# pass-1 scan_locals (the if-expr's then-arm reads raw_sub but
# raw_sub is still in the deferred-declaration window), then
# stuck there because the pass-2 merge had no "concrete → poly"
# rule. The codegen-emitted assignment then puts an sp_RbVal
# poly into the typed slot, failing C compile with
# `incompatible types when assigning to type 'sp_StrIntHash *'
#  from type 'sp_RbVal'`.

class T_hash_fetch_hash_default_narrowing_ArticleParams
  def self.from_raw(params)
    raw_sub = params.fetch "article", {}
    sub = if raw_sub.is_a?(Hash)
      raw_sub
    else
      {}
    end
    sub.is_a?(Hash) ? "got-hash" : "no-hash"
  end
end

puts T_hash_fetch_hash_default_narrowing_ArticleParams.from_raw({ "x" => 1 })
puts T_hash_fetch_hash_default_narrowing_ArticleParams.from_raw({ "y" => 2 })

# === hash_fetch_string_default_on_int_leaf ===
# #454: `params.fetch "k", ""` on a Hash[String, Int] (or
# Hash[Symbol, Int]) emitted a type-mismatched ternary
# (`has_key ? get_int : char_star_default`) and failed C compile
# under -Werror=int-conversion.
#
# Narrow fix: when the default is a string literal and the hash
# leaf is int, widen the call's static return type to "string"
# and route the get arm through sp_int_to_s so both arms agree
# on `const char *`. Limited to (int leaf + string default);
# broader (poly leaf, hash default, etc.) cascades through the
# `is_a?(Hash)` narrowing in real-blog params and is left for
# a follow-up.

class T_hash_fetch_string_default_on_int_leaf_P
  def self.lookup_str(params)
    params.fetch "title", ""
  end

  def self.lookup_sym(params)
    params.fetch :title, ""
  end
end

# Hit path: the integer at the key gets stringified.
puts T_hash_fetch_string_default_on_int_leaf_P.lookup_str({ "x" => 1, "title" => 42 })
# Miss path: the empty-string default.
puts T_hash_fetch_string_default_on_int_leaf_P.lookup_str({ "x" => 1 })

# Sym-keyed counterpart.
puts T_hash_fetch_string_default_on_int_leaf_P.lookup_sym({ x: 1, title: 99 })
puts T_hash_fetch_string_default_on_int_leaf_P.lookup_sym({ x: 1 })

# Matched-default control (no fix needed; existing behavior).
class T_hash_fetch_string_default_on_int_leaf_Q
  def self.lookup(params)
    params.fetch "x", 0
  end
end
puts T_hash_fetch_string_default_on_int_leaf_Q.lookup({ "x" => 7 })
puts T_hash_fetch_string_default_on_int_leaf_Q.lookup({ "other" => 1 })

# === heterogeneous_dispatch_int_return_narrow ===
# Approach-2 narrowing: when `<poly>[k]` chases an `arr[i]` whose
# `arr` is a poly_array with observed elements that all return int
# from `[]` (IntArray + Method-objects), narrow the outer dispatch
# return type from poly to int. Without narrowing, the outer call
# returns sp_RbVal and downstream sites — `total + 1`, `~bits`,
# `iv += 1`, etc. — fail the C compile or cascade widen ivars to
# poly.

class T_heterogeneous_dispatch_int_return_narrow_Box
  def initialize
    @slots = [nil] * 4         # int_array initially
    @int_arr = [10, 20, 30, 40]
    @slots[0] = method(:double_at)   # widens via obj_Method_ptr_array
    @slots[1] = @int_arr             # then widens to poly_array
    @total = 0
  end

  def double_at(i)
    i * 2
  end

  # Direct ivar access: narrowing fires via `InstanceVariableReadNode`.
  def via_ivar(i)
    @slots[i][i]
  end

  # Local-variable alias: narrowing fires via `LocalVariableReadNode`
  # with the AST-walking fallback in `find_lv_ivar_alias_in_ast`.
  def via_local(i)
    cache = @slots
    cache[i][i]
  end

  def run
    # IntArray arm of @slots[1]: returns @int_arr[1] = 20.
    @total = via_ivar(1)
    @total += 1
    puts @total                # 21
    # Method arm of @slots[0]: returns double_at(0) = 0.
    @total = via_local(0)
    @total += 1
    puts @total                # 1
  end
end

T_heterogeneous_dispatch_int_return_narrow_Box.new.run

# === if_static_false_skips_dead_branch_compile ===
# `compile_cond_expr` already returns the literal "FALSE" for a
# predicate whose static type is `nil` (e.g. an attr-style read
# of an ivar only ever assigned `nil`). The if/unless emit then
# wraps the dead body in `if (FALSE) { … }`, but the body's C
# statements still pass through gcc — and they often type-error
# against ivars/methods whose shapes only make sense when the
# predicate is true. Skipping body emission lets the rest of
# the program compile.

class T_if_static_false_skips_dead_branch_compile_Profiler
  def initialize
    # `@mode` is statically nil — only ever assigned this literal.
    # The read in the predicate below folds to `if (FALSE)`.
    @mode = nil
  end

  def run
    # Dead branch when @mode is nil. Pre-fix, spinel emits the
    # body anyway: `sp_str_sub("...", "MODE", iv_mode)` with
    # iv_mode typed as `mrb_int` (its only init being `= nil`)
    # — the 3rd arg fails -Wint-conversion.
    if @mode
      out = "label_MODE".sub("MODE", @mode)
      puts out
    end
    puts "ran"
  end
end

# `if @nil_ivar; ... else; ... end` with a trailing stmt forces
# the if into statement position (compile_if_stmt). The else arm
# is the live one when the predicate is statically nil.
class T_if_static_false_skips_dead_branch_compile_Either
  def initialize
    @verbose = nil
  end

  def run
    if @verbose
      out = "label_MODE".sub("MODE", @verbose)
      puts out
    else
      puts "quiet"
    end
    puts "done"
  end
end

T_if_static_false_skips_dead_branch_compile_Profiler.new.run
T_if_static_false_skips_dead_branch_compile_Either.new.run

# === imeth_name_collision_recv_aware ===
# Issue #429. Two unrelated classes each defining `def get(k)`
# with different return types caused the analyzer's int-recv
# cross-class widening to pick the first match -- so a local
# `r = c.get("/foo")` where c is statically obj_IntClient ended
# up declared `const char *` (from T_imeth_name_collision_recv_aware_StrBag#get's String return),
# crashing the C compile when sp_IntClient_get returned an
# sp_IntBag *.
#
# Fix: scan_locals' first pass calls infer_type before the
# scope decls land, so `c`'s var-type is "" / "int" at that
# point. infer_recv_method_type's int-recv arm enumerates
# every user class with `mname` and picks the first non-int
# return. The fix bails out of that path when (a) the recv is
# a LocalVariableReadNode whose var-type isn't pinned yet, and
# (b) the candidate classes disagree on the return type --
# leaving a later iteration of the iterative loop to pick the
# right one via the is_obj_type arm once `c`'s declaration
# has propagated.
#
# Coverage: the canonical "two classes, same imeth name,
# different return types, statically-typed call sites for
# each" shape from Ori's repro.

class T_imeth_name_collision_recv_aware_StrBag
  def initialize
    @h = {"a" => "b"}
    @h.delete("a")
    @h["x"] = "got-x"
  end
  def get(k)
    @h[k]
  end
end

class T_imeth_name_collision_recv_aware_IntBag
  attr_accessor :status
  def initialize
    @status = 42
  end
end

class T_imeth_name_collision_recv_aware_IntClient
  def get(path)
    out = T_imeth_name_collision_recv_aware_IntBag.new
    out.status = path.length
    out
  end
end

s = T_imeth_name_collision_recv_aware_StrBag.new
puts s.get("x")

c = T_imeth_name_collision_recv_aware_IntClient.new
r = c.get("/foo")
puts r.status


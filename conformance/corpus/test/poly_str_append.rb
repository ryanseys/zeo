# Issue #313: a poly-typed value flowing into `String#<<` (mutable_str)
# used to emit `sp_String_append(io, sp_RbVal_value)` directly, which
# C-rejects (`const char *` parameter vs `sp_RbVal` argument). The
# canonical shape: a module-level method that returns
# `@hash[k] || ""` over a heterogeneously-valued hash has return type
# `sp_RbVal` per #303, and `io << M.get(:t)` then trips the bug.
#
# Fix: at the `mutable_str << poly` codegen site (both expression-
# and statement-context emitters), wrap the arg in sp_poly_to_s
# before calling sp_String_append. sp_poly_to_s coerces every tag —
# strings pass through, ints stringify, sym/nil/etc fall through.
#
# Sister fix to #303 — that one made the *hash* poly when written
# heterogeneously; this one handles the *read path* flowing into a
# narrow callee.

module M
  @data = {}

  def self.set(k, v)
    @data[k] = v
  end

  def self.get(k)
    @data[k] || ""
  end
end

# Mixed value types force the hash to be inferred as poly per #303.
M.set(:n, 42)
M.set(:t, "hello")

io = String.new
io << M.get(:t)
puts io                # hello

# Also exercise the expression-context << path via chained append.
io2 = String.new
(io2 << M.get(:t)) << M.get(:t)
puts io2               # hellohello

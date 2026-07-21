# Issue #236 follow-up to #234: chained `@a = @b = {}` (or `[]`)
# with slots promoted to a typed container (e.g. `str_str_hash`)
# emitted a default `sp_StrIntHash *` temp and assigned it to both
# slots, mismatching the promoted slot's pointer type. The fix
# detects empty-literal RHS in compile_chained_ivar_writes and,
# when every chain participant agrees on the same promoted type,
# emits the slot-typed constructor for the temp so the assignment
# type-checks and Ruby's "one shared object" semantic is preserved
# (both slots receive the same pointer).

# Hash: both slots promoted to str_str_hash via @h["k"] = "v" writes.
class HashOwner
  def initialize
    @a = {}
    @a["seed"] = "value"
    @b = {}
    @b["seed"] = "value"
  end

  def reset
    @a = @b = {}
  end

  # Sharing test: write to @a, observe through @b. Pre-fix this
  # wouldn't compile; if the chain helper accidentally regressed to
  # per-slot constructors, sharing would be lost and @b would still
  # report length 0 after the @a write.
  def post_reset_share_check
    @a["after"] = "ok"
    puts @b.length            # 1 if shared, 0 if separate objects
    puts @b["after"]          # "ok" if shared
  end
end

ho = HashOwner.new
ho.reset
ho.post_reset_share_check

# Array: both slots promoted to str_array via push("x"/"y").
class ArrayOwner
  def initialize
    @a = []
    @a.push("x")
    @b = []
    @b.push("y")
  end

  def reset
    @a = @b = []
  end

  def post_reset_share_check
    @a.push("z")
    puts @b.length            # 1 if shared
    puts @b[0]                # "z" if shared
  end
end

ao = ArrayOwner.new
ao.reset
ao.post_reset_share_check

# Three-chain to confirm the consensus check covers length-3 chains.
class ThreeChain
  def initialize
    @a = {}; @a["s"] = "v"
    @b = {}; @b["s"] = "v"
    @c = {}; @c["s"] = "v"
  end

  def reset
    @a = @b = @c = {}
  end

  def show
    puts @a.length
    puts @b.length
    puts @c.length
  end
end

tc = ThreeChain.new
tc.reset
tc.show

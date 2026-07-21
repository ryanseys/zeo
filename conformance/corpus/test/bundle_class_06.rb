# Bundled tests:
#   - each_block_cmeth_widen
#   - each_poly_recv_block_auto_splat
#   - empty_hash_ivar_string_value
#   - ensure_raise_overrides_body_raise
#   - ensure_runs_on_raise

# === each_block_cmeth_widen ===
# Issue #424. `T_each_block_cmeth_widen_Json.escape(k)` inside `h.each |k, v|` couldn't
# pick up k's narrowed string type from the hash variant -- the
# cmeth param `s` stayed at the int default and the C compile
# of the call site failed with `passing 'const char *' to
# parameter of type 'mrb_int'`. Fix runs a targeted pre-scan
# (widen_cmeths_via_hash_each_blocks) that walks hash-typed
# params for `<p>.each |k, v|` blocks and widens any nested
# `<Class>.<cmeth>(k|v)` call's param types from the hash's
# key/value variant.
#
# Coverage:
#   - str_str_hash: k and v both string; escape's param s
#     widens to string via both call sites.
#   - str_int_hash variant: k string, v int. Mixed types
#     force the cmeth widening to handle each arg independently.

class T_each_block_cmeth_widen_Json
  def self.escape(s)
    out = ""
    i = 0
    n = s.length
    while i < n
      out = out + s[i]
      i += 1
    end
    out
  end

  def self.from_str_hash(h)
    out = "{"
    h.each do |k, v|
      out = out + "\"" + T_each_block_cmeth_widen_Json.escape(k) + "\":\"" + T_each_block_cmeth_widen_Json.escape(v) + "\""
    end
    out + "}"
  end
end

puts T_each_block_cmeth_widen_Json.from_str_hash({"a" => "b"})
puts T_each_block_cmeth_widen_Json.from_str_hash({"x" => "y"})

# === each_poly_recv_block_auto_splat ===
# `arr.each {|a, b| ... }` over a poly receiver (an sp_RbVal that
# carries an array of arrays at runtime) should auto-splat each
# element across the block params, matching CRuby's
# `arr.each {|*args| a, b, *_ = args }` shape.  Pre-fix the poly
# branch only assigned `bp1` and dropped `bp2`, so e.g.
# `[[1, 2], [3, 4]].each {|a, b| sum += a + b }` skipped `b`
# entirely.  Cover both inner kinds the destructuring uses:
# poly_array (heterogeneous element types) and int_array
# (homogeneous int element types — what `[i, fixed]` lowers to in
# optcarrot's `setup_lut`).

class T_each_poly_recv_block_auto_splat_C
  def initialize
    @h = {}
  end

  def push_int(bank, pair)
    # `pair` lowers to int_array because both elements are ints.
    (@h[bank] ||= []) << pair
  end

  def push_poly(bank, pair)
    # `pair` lowers to poly_array because the elements are mixed.
    (@h[bank] ||= []) << pair
  end

  def each_pair(bank)
    arr = @h[bank]
    sum = 0
    arr.each {|a, b| sum += a.to_i + b.to_i }
    sum
  end
end

c = T_each_poly_recv_block_auto_splat_C.new
c.push_int(0, [10, 20])
c.push_int(0, [3,  4])
puts c.each_pair(0)         # 10 + 20 + 3 + 4 = 37

d = T_each_poly_recv_block_auto_splat_C.new
d.push_poly(1, [100, "x"])  # b is a string, exercising the poly_array branch
d.push_poly(1, [5,   "y"])
puts d.each_pair(1)         # 100 + 0 + 5 + 0 = 105 (string `to_i` of "x" / "y" = 0)

# === empty_hash_ivar_string_value ===
# Issue #64: an ivar initialized as `{}` defaulted to `str_int_hash`,
# and a later `@h[k] = "string"` write fed `const char *` into
# `sp_StrIntHash_set` (which expects `mrb_int`). Three pieces of the
# fix:
#   - scan_writer_calls now recognises `@h[k] = v` against an ivar
#     still typed as the empty-hash default and promotes the slot
#     based on the actual key/value types
#   - the same scanner skips empty `{}` / `[]` writes when re-scanning
#     so the iterative loop doesn't widen the promoted type back to
#     poly via "old=str_str_hash, new=str_int_hash"
#   - both compile_stmt(InstanceVariableWriteNode) and the inline
#     constructor walker route empty `{}` against a promoted ivar to
#     the matching `sp_*Hash_new()` ctor

class T_empty_hash_ivar_string_value_Sources
  def initialize
    @file_sources = {}
    @current_file = "a.rb"
  end

  def add(source)
    @file_sources[@current_file] = source if @current_file
  end

  def lookup(name)
    @file_sources[name]
  end
end

s = T_empty_hash_ivar_string_value_Sources.new
s.add("body")
puts s.lookup("a.rb")        # body
puts s.lookup("missing")     # (empty line — string hash default for missing key)

# The other key/value-type combos covered by the same promotion path.
class T_empty_hash_ivar_string_value_IntKeyed
  def initialize; @h = {}; end
  def put(k, v); @h[k] = v; end
  def get(k); @h[k]; end
end

ik = T_empty_hash_ivar_string_value_IntKeyed.new
ik.put(7, "seven")
ik.put(42, "forty-two")
puts ik.get(7)               # seven
puts ik.get(42)              # forty-two

# Symbol-keyed promotion is also wired through but exercises a
# pre-existing -Walloc-size-larger-than warning in the runtime
# `sp_SymIntHash_grow` (unrelated to this issue), so it isn't part
# of the make-test surface here.

# === ensure_raise_overrides_body_raise ===
# Ruby semantics: when both the body and the ensure clause raise,
# the ensure-raise wins — the original body exception is replaced
# by the ensure exception as it propagates.

class T_ensure_raise_overrides_body_raise_C
  def f
    begin
      raise "original"
    ensure
      raise "from-ensure"
    end
  end
end

begin
  T_ensure_raise_overrides_body_raise_C.new.f
rescue => e
  puts e
end

# === ensure_runs_on_raise ===
# When the body of a `begin..ensure..end` raises, the ensure
# clause must still run before the exception keeps propagating.
# Previously the generated T_ensure_runs_on_raise_C had no setjmp around the body, so
# raise unwound past the ensure entirely.

class T_ensure_runs_on_raise_C
  def initialize
    @cleanup = "no"
    @before_raise = 0
  end

  def f
    begin
      @before_raise = 1
      raise "boom"
    ensure
      @cleanup = "yes"
    end
  end

  def report
    begin
      f
    rescue => e
      puts e
    end
    puts @before_raise
    puts @cleanup
  end
end

T_ensure_runs_on_raise_C.new.report


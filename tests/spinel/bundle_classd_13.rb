# Bundled tests:
#   - nested_proc_interp_capture
#   - new_kwarg_bundle
#   - nil_ivar_no_predicate_stays_scalar
#   - nil_scalar_ivar_widen
#   - no_attr_write_shortcut_complex

# === nested_proc_interp_capture ===
# Nested blocks that capture a variable through string interpolation
# inside a block that itself captures the same variable.
#
# This exercises the @nd_parts recursion fix in scan_lambda_free_vars:
# LocalVariableReadNode("i") is nested under
#   InterpolatedStringNode → @nd_parts → EmbeddedStatementsNode → body
# and without @nd_parts recursion the variable is never detected as free.

class T_nested_proc_interp_capture_Defer
  def initialize
    @cnt = 0
  end

  def add(&blk)
    if @cnt == 0
      @blk0 = blk
    elsif @cnt == 1
      @blk1 = blk
    else
      @blk2 = blk
    end
    @cnt += 1
    nil
  end

  def call
    @blk2.call
    @blk1.call
    @blk0.call
  end
end

class T_nested_proc_interp_capture_Runner
  def self.with_deferred(&outer)
    d = T_nested_proc_interp_capture_Defer.new
    outer.call(d)
    d.call
  end
end

T_nested_proc_interp_capture_Runner.with_deferred do |d|
  i = 1
  d.add { puts "first #{i}" }
  i += 1
  d.add { puts "second #{i}" }
  puts "mid"
  i += 1
  d.add { puts "third #{i}" }
end

# === new_kwarg_bundle ===
# #530. `Class.new(kw: val)` (Symbol-keyed kwargs) where the
# class's initialize takes a single positional `attrs` param.
# CRuby binds `attrs = {kw: val}` (the kwargs become a positional
# hash whose keys are the kwarg *symbols*). Spinel previously
# emitted `sp_Foo_new(0)` because the kwarg name didn't match the
# positional param name -- the kwarg got silently dropped and the
# param defaulted to int.
#
# Fix: when a KeywordHashNode arg's keys match no param name
# and there's still an unfilled positional, treat the whole
# hash as that positional's value. analyze widens the param's
# inferred type to `sym_poly_hash` (symbol keys, poly values for
# kwarg-value variety) and codegen builds the hash from the
# kwargs at the call site. Access is via the kwarg symbols
# (`attrs[:title]`); string keys would not match, matching CRuby.

class T_new_kwarg_bundle_Article
  def initialize(attrs)
    @title = attrs[:title]
    @body  = attrs[:body]
  end
  attr_reader :title, :body
end

a = T_new_kwarg_bundle_Article.new(title: "Hello", body: "World")
puts a.title
puts a.body

# Two kwarg call sites with different value shapes -- both flow
# into the same sym_poly_hash widening.
b = T_new_kwarg_bundle_Article.new(title: "Second", body: "Post")
puts b.title

# T_new_kwarg_bundle_Mixed value types in one kwarg call: the sym_poly_hash carries
# all the boxed values; consumers unbox at the use site.
class T_new_kwarg_bundle_Mixed
  def initialize(opts)
    @s = opts[:name]
    @n = opts[:count]
  end
  attr_reader :s, :n
end

m = T_new_kwarg_bundle_Mixed.new(name: "abc", count: 42)
puts m.s
puts m.n.inspect

# === nil_ivar_no_predicate_stays_scalar ===
# #497. Regression guard for the optcarrot APU::Pulse shape that
# motivated reverting #495: an ivar initialized to `nil` and later
# used purely as int arithmetic (no `.nil?` read anywhere on it)
# must stay at `int` storage. The pre-revert widening cascaded
# `@wave_length` to poly via scan_writer_calls's new nil branch,
# which then forced `@freq = (@wave_length + 1) * 2 * @fixed`
# to a poly expression and broke every downstream `iv_freq`,
# `iv_timer`, `iv_step` arithmetic site under -Werror.
#
# Test asserts both program output AND the implicit -Werror=int-
# conversion compile (the harness drops cc stderr but a widened
# ivar would surface as a missing binary / wrong output).

class T_nil_ivar_no_predicate_stays_scalar_APUFake
  def initialize
    @wave_length = nil
    @fixed = 1
    @freq = 0
    @timer = 0
    @step = 0
  end

  def configure(n)
    @wave_length = n
    @freq = (@wave_length + 1) * 2 * @fixed
    @timer = @freq
  end

  def step!
    @step = (@step + 1) & 7
    @timer += @freq
    @step
  end

  def report
    @step.to_s + "," + @timer.to_s
  end
end

a = T_nil_ivar_no_predicate_stays_scalar_APUFake.new
a.configure(3)
puts a.report
a.step!
a.step!
puts a.report

# === nil_scalar_ivar_widen ===
class T_nil_scalar_ivar_widen_HitState
  def initialize
    @material_index = nil
  end

  def hit!(idx)
    @material_index = idx
  end

  def label
    @material_index.nil? ? "miss" : "hit"
  end
end

state = T_nil_scalar_ivar_widen_HitState.new
puts state.label
state.hit!(0)
puts state.label

class T_nil_scalar_ivar_widen_ResetState
  def initialize
    @material_index = 0
  end

  def clear!
    @material_index = nil
  end

  def label
    @material_index.nil? ? "miss" : "hit"
  end
end

reset = T_nil_scalar_ivar_widen_ResetState.new
puts reset.label
reset.clear!
puts reset.label

class T_nil_scalar_ivar_widen_ParamResetState
  def initialize
    @material_index = 0
  end

  def set(idx)
    @material_index = idx
  end

  def label
    @material_index.nil? ? "miss" : "hit"
  end
end

param_reset = T_nil_scalar_ivar_widen_ParamResetState.new
puts param_reset.label
param_reset.set(nil)
puts param_reset.label

# === no_attr_write_shortcut_complex ===
# `def x=(v); @x = v * 2; end` looks like an attr_writer at first glance
# (single InstanceVariableWriteNode body) but the assignment value is a
# computation, not a bare param reference — `is_simple_writer_method`
# must not classify it as auto-attr_writer.
#
# Without that fix, the method gets auto-registered in @cls_attr_writers,
# `cls_has_attr_writer(T_no_attr_write_shortcut_complex_C, "doubled")` returns true, and the call site
# short-circuits `c.doubled = 5` to `c->iv_doubled = 5` — bypassing the
# `* 2` entirely. Ruby would print 10; pre-fix Spinel printed 5.

class T_no_attr_write_shortcut_complex_C
  def initialize
    @doubled = 0
  end

  def doubled=(v)
    @doubled = v * 2
  end

  def get_doubled
    @doubled
  end
end

c = T_no_attr_write_shortcut_complex_C.new

c.doubled = 5
puts c.get_doubled    # 10  (5 * 2)

c.doubled = -7
puts c.get_doubled    # -14

puts "done"


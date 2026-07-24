# Bundled tests:
#   - cvar_op_write
#   - def_aset_mixed_ivar_widen
#   - default_argv_narrows_from_string_call_site
#   - default_nil_param_with_hash_lookup
#   - default_nil_param_with_sym_hash_lookup

# === cvar_op_write ===
# Issue #398: `@@x op= rhs` was rejected as unsupported syntax
# (`ClassVariableOperatorWriteNode` had no parser/codegen handling).
# `@@x = @@x op rhs` worked. Same gap for `||=` / `&&=`. Adds the
# parser cases (PM_CLASS_VARIABLE_{OPERATOR,OR,AND}_WRITE_NODE) and
# codegen arms (compile_stmt, compile_expr, compile_body_return).
#
# Limited to type-compatible cvar slots: cvar widening (e.g.
# `@@x = nil; @@x ||= "str"` needing the slot to widen from int
# to string) is a separate issue (sister to ivar widening at
# scan_ivars).

class T_cvar_op_write_Counter
  @@count = 0
  @@total = 100
  @@flag = 1

  def self.bump
    @@count += 1
    @@total += 10
  end

  def self.shift
    @@count <<= 2
  end

  def self.zero_flag
    @@flag &&= 7
  end

  def self.report
    puts @@count
    puts @@total
    puts @@flag
  end
end

T_cvar_op_write_Counter.bump
T_cvar_op_write_Counter.bump
T_cvar_op_write_Counter.bump
T_cvar_op_write_Counter.report     # 3, 130, 1

T_cvar_op_write_Counter.shift
T_cvar_op_write_Counter.report     # 12, 130, 1

T_cvar_op_write_Counter.zero_flag  # @@flag is truthy (1), so &&= writes 7
T_cvar_op_write_Counter.report     # 12, 130, 7

# === def_aset_mixed_ivar_widen ===
# `def []=(name, value)` whose body dispatches on `name` and writes
# `value` into ivars of differing types (`@id = value` and
# `@name = value`, with `@id: mrb_int` and `@name: const char *`).
# Pre-fix the param was pinned at `mrb_int` from the initialize-time
# observation, and the second ivar assignment errored with
# "incompatible integer to pointer conversion".
#
# widen_param_types_from_body_writes now also looks at
# InstanceVariableWriteNode whose RHS is the param: when two or
# more distinct ivar slot types are observed across such writes
# within a `def []=` body, the param widens to poly. Each branch
# can then unbox / box as the slot demands.

class T_def_aset_mixed_ivar_widen_Foo
  def initialize
    @id = 0
    @name = ""
  end

  def []=(name, value)
    case name
    when :id
      @id = value
    when :name
      @name = value
    end
  end

  def id
    @id
  end

  def name
    @name
  end
end

f = T_def_aset_mixed_ivar_widen_Foo.new
f[:id] = 42
f[:name] = "hello"
puts f.id
puts f.name

# === default_argv_narrows_from_string_call_site ===
# `def initialize(conf = ARGV)` typed `conf` as the specialised
# `argv` scalar (because `infer_type(ARGV) == "argv"`). A caller
# that passes a single string then unified the call-site `string`
# against `argv` — the unifier had no argv-vs-string rule, so it
# dropped to the catch-all poly tail. `conf` widened to poly and
# `T_default_argv_narrows_from_string_call_site_Wrapper.new(conf)` (which expects a String) received a poly arg,
# miscompiling the inner `@s.length` read.
#
# Narrowing argv + string → string biases toward the call-site
# shape: single-string entry points don't drag the whole signature
# into poly, while genuinely-array-of-strings call sites still
# unify on their own type via the existing array-array path.

class T_default_argv_narrows_from_string_call_site_Wrapper
  def initialize(s)
    @s = s
  end
  def show
    puts @s.length
  end
end

class T_default_argv_narrows_from_string_call_site_Entry
  def initialize(conf = ARGV)
    @conf = T_default_argv_narrows_from_string_call_site_Wrapper.new(conf)
  end
  def show
    @conf.show
  end
end

T_default_argv_narrows_from_string_call_site_Entry.new("hello").show     # 5

# === default_nil_param_with_hash_lookup ===
# #482. `def m(other = nil)` whose body uses `other[key]` (Hash
# receiver index read) failed C compile when the lookup result
# landed in a concretely-typed pointer field. With no caller
# passing a non-nil `other`, spinel typed the param at the
# `mrb_int` default; `other["key"]` fell through to the
# unresolved-call warning (emits 0); and the resulting `lv_v`
# assigned to the typed `iv_s` slot tripped
# -Wint-conversion. Both the `return if other.nil?` and the
# `if !v.nil?` guards folded away because spinel reasoned about
# `mrb_int 0` as Integer 0 whose `.nil?` is false.
#
# Fix: a new back-propagation pass detects an int-typed param
# with `nil` default whose body uses it as a String-keyed Hash
# receiver, and widens the param's stored type from int to
# str_str_hash. Spinel already treats hash pointers as nullable,
# so the early-return / `.nil?` checks survive DCE.

class T_default_nil_param_with_hash_lookup_Box
  attr_accessor :s

  def initialize(other = nil)
    @s = nil
    return if other.nil?
    v = other["key"]
    @s = v if !v.nil?
  end
end

b = T_default_nil_param_with_hash_lookup_Box.new
b.s = "hello"
puts b.s

# === default_nil_param_with_sym_hash_lookup ===
# Sym-keyed sibling of #482. Same shape — nil-default param +
# body uses param as Hash receiver — but with a SymbolNode key
# instead of a StringNode key. The widening pass now distinguishes
# between str / sym keys and picks sym_str_hash instead of
# str_str_hash when the body's `param[<key>]` carries a symbol.

class T_default_nil_param_with_sym_hash_lookup_Box
  attr_accessor :s
  def initialize(other = nil)
    @s = nil
    return if other.nil?
    v = other[:key]
    @s = v if !v.nil?
  end
end

b = T_default_nil_param_with_sym_hash_lookup_Box.new
b.s = "hello"
puts b.s


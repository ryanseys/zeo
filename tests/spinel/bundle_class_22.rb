# Bundled tests:
#   - ivar_hash_aset_param_widen
#   - ivar_lazy_init_push_via_getter
#   - ivar_op_assign_bitwise_and_shift
#   - ivar_or_write_basic
#   - ivar_same_shape_string_new_no_widen

# === ivar_hash_aset_param_widen ===
# #488. A class method writing a param into a typed ivar hash —
# `@data[key] = value` where @data is pinned to e.g. str_str_hash
# by another method — left the writer's `key` and `value` params
# at the `mrb_int` default. The C emit then passed mrb_int into
# the const char * key / value slots and failed -Wint-conversion.
# Sibling to #482's nil-default + Hash-receiver back-propagation
# pass; this one covers the @hash-write side of the same
# back-propagation gap.

class T_ivar_hash_aset_param_widen_Bag
  def initialize
    @data = {}
  end

  # Pins @data to str_str_hash from a typed source.
  def populate(source)
    source.each do |k, v|
      @data[k] = v
    end
  end

  # No caller; without back-propagation `key` and `value` stayed
  # at mrb_int and the body's sp_StrStrHash_set fired
  # -Wint-conversion.
  def []=(key, value)
    @data[key] = value
  end

  def fetch(k)
    @data[k]
  end
end

b = T_ivar_hash_aset_param_widen_Bag.new
b.populate({ "x" => "hello" })
puts "ok"
puts b.fetch("x")

# === ivar_lazy_init_push_via_getter ===
# Issue #430. `@ivar = [] if @ivar.nil?` plus push-through-getter:
# the ivar stayed inferred as IntArray (from the empty `[]`
# literal) even when pushes through a getter method handed in
# string values. sp_IntArray_push then took the string as a
# mrb_int -- C compile error.
#
# Fix: scan_writer_calls now also matches the bare-call shape
# `<getter>(*args) << v` / `<getter>.push(v)` by resolving
# through method_returns_ivar_in_class -- if the getter method's
# body returns a bare `@<iname>` as its last expression, the
# push observation lands on `@<iname>` directly. The empty-array
# default promotes the same way it does for direct `@x.push`
# / `@x << v` writes.
#
# Coverage:
#   - The canonical Rails-style `ActiveRecord::Base#errors`
#     shape: lazy-init via nil guard, push via a sibling
#     `add_error` method that calls `errors << "..."`.
#   - Same shape with `self.<getter>` (explicit self-recv) so
#     both the bare and self-prefixed call forms route to the
#     ivar.

class T_ivar_lazy_init_push_via_getter_Errors
  def list
    @list = [] if @list.nil?
    @list
  end

  def add(msg)
    list << msg
  end
end

class T_ivar_lazy_init_push_via_getter_ErrorsSelf
  def list
    @list = [] if @list.nil?
    @list
  end

  def add(msg)
    self.list << msg
  end
end

e = T_ivar_lazy_init_push_via_getter_Errors.new
e.add("a")
e.add("b")
puts e.list.length
puts e.list[0]
puts e.list[1]

es = T_ivar_lazy_init_push_via_getter_ErrorsSelf.new
es.add("x")
puts es.list[0]

# === ivar_op_assign_bitwise_and_shift ===
# Statement-form `@x OP= v` for the bitwise / shift / multiplicative
# operators. The pre-fix codegen only handled `+=` and `-=`; every
# other op-assign was silently dropped, so `@a &= 0x80` left @a
# untouched. Walks all the operators that map onto T_ivar_op_assign_bitwise_and_shift_C `OP=` directly.

class T_ivar_op_assign_bitwise_and_shift_C
  attr_reader :a
  def initialize; @a = 0xff; end
  def m_amp; @a &= 0x80; end
  def m_or;  @a |= 0x01; end
  def m_xor; @a ^= 0xff; end
  def m_lsh; @a <<= 1; end
  def m_rsh; @a >>= 1; end
  def m_mul; @a *= 2; end
end

c = T_ivar_op_assign_bitwise_and_shift_C.new; c.m_amp; puts "amp: #{c.a}"
c = T_ivar_op_assign_bitwise_and_shift_C.new; c.m_or;  puts "or: #{c.a}"
c = T_ivar_op_assign_bitwise_and_shift_C.new; c.m_xor; puts "xor: #{c.a}"
c = T_ivar_op_assign_bitwise_and_shift_C.new; c.m_lsh; puts "lsh: #{c.a}"
c = T_ivar_op_assign_bitwise_and_shift_C.new; c.m_rsh; puts "rsh: #{c.a}"
c = T_ivar_op_assign_bitwise_and_shift_C.new; c.m_mul; puts "mul: #{c.a}"

# === ivar_or_write_basic ===
class T_ivar_or_write_basic_T
  def initialize
    @str = nil
    @hits = 0
    @marker = "init"
  end

  def cache
    @str ||= compute
  end

  def compute
    @hits += 1
    "hello"
  end

  def get
    @str
  end

  def hits
    @hits
  end
end

t = T_ivar_or_write_basic_T.new
puts t.get.nil? ? "nil-before" : "not-nil-before"
puts t.cache
puts t.get
puts t.cache
puts t.get
puts "hits=#{t.hits}"

class T_ivar_or_write_basic_U
  def initialize
    @v = nil
    @hits = 0
    @log = []
  end

  def value
    @v ||= 42
  end

  def hits
    @hits
  end

  def log_size
    @log.size
  end
end

u = T_ivar_or_write_basic_U.new
puts u.value
puts u.value
puts u.value

# === ivar_same_shape_string_new_no_widen ===
# Two `String.new` writes to the same ivar should keep the slot at
# `sp_String *`, not widen to `sp_RbVal`. Pre-fix scan_ivars used
# `infer_ivar_init_type` which returned `obj_String` for the first
# write; later writer-scan saw `mutable_str` via `infer_type` for the
# second; update_ivar_type took the disagreement as a real conflict
# and widened to poly. Issue #629.

class T_ivar_same_shape_string_new_no_widen_T
  def initialize
    @body = String.new
  end

  def reset
    @body = String.new
  end

  def append(s)
    @body << s
  end

  def body
    @body
  end
end

t = T_ivar_same_shape_string_new_no_widen_T.new
t.append("hello")
puts t.body
t.reset
t.append("world")
puts t.body


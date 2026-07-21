# Bundled tests:
#   - forward_call_param_type_obj
#   - gc_save_double_in_new
#   - gc_scan_skip_inherited_ivar
#   - hash_each_param_narrow
#   - hash_fetch_hash_default

# === forward_call_param_type_obj ===
# Forward-call with a user-class-typed arg flowing through the
# caller's ivar to the callee's parameter slot. Mirrors optcarrot's
# `@ppu.nametables = @mirroring` shape — the callee accepts an
# `obj_<UserClass>` and the int→class fallback dispatches via the
# yet-untyped `@ppu` ivar.

class T_forward_call_param_type_obj_Holder
  def initialize(name)
    @name = name
  end
  def label
    "h:#{@name}"
  end
end

class T_forward_call_param_type_obj_Caller
  def initialize(target)
    @target = target
    @holder = T_forward_call_param_type_obj_Holder.new("alpha")
  end
  def go
    @target.attach(@holder)
  end
end

class T_forward_call_param_type_obj_Target
  def attach(h)
    @holder = h
  end
  def held_label
    @holder.label
  end
end

t = T_forward_call_param_type_obj_Target.new
c = T_forward_call_param_type_obj_Caller.new(t)
c.go
puts t.held_label

# === gc_save_double_in_new ===
class T_gc_save_double_in_new_GcSaveDoubleInNew
  def initialize(path)
    data = [1, 2, 3]
    @x = data
  end

  def step
    @x.length
  end
end

o = T_gc_save_double_in_new_GcSaveDoubleInNew.new("a")
puts o.step

# === gc_scan_skip_inherited_ivar ===
# When parent and child both record the same ivar name, the struct
# field is contributed once by `emit_parent_fields`. `emit_class_struct`
# already skips child-side redeclaration, so child's own gc_scan walk
# must also skip the inherited slot — otherwise it emits a second
# (often miscast) mark on top of the parent walk's correct mark.
#
# Here parent's `@data` is widened to poly (heterogeneous writes), and
# child also writes `@data` from a method-returned poly value, exercising
# the inherited-slot path under GC pressure.

class T_gc_scan_skip_inherited_ivar_Base
  def initialize(kind)
    if kind == 0
      @data = [1, 2, 3]
    else
      @data = "hello"
    end
  end
end

class T_gc_scan_skip_inherited_ivar_Holder
  def initialize(v)
    @v = v
  end
  attr_reader :v
end

class T_gc_scan_skip_inherited_ivar_Child < T_gc_scan_skip_inherited_ivar_Base
  def initialize(h)
    super(0)
    @data = h.v
  end
end

class T_gc_scan_skip_inherited_ivar_Trash
  def initialize(n)
    @n = n
    @s = "padding payload " * 64
  end
end

h_str = T_gc_scan_skip_inherited_ivar_Holder.new("from-holder")
T_gc_scan_skip_inherited_ivar_Holder.new(123)
c = T_gc_scan_skip_inherited_ivar_Child.new(h_str)

junk = []
i = 0
while i < 5000
  junk << T_gc_scan_skip_inherited_ivar_Trash.new(i)
  i = i + 1
end

puts junk.length
puts "ok"

# === hash_each_param_narrow ===
# Issue #408. Regression guard for the hash-each body-walker
# narrow path (9ca01d7 + analyze-side port in 02003ad).
#
# Pre-fix shape: a class method whose only signal on its hash
# param is `h.each |k, v|` inside the body left `h` typed as
# poly_poly_hash, made `k` / `v` poly, and the str-concat
# `\"\\\"\" + k + \"\\\":\"` cascaded into compile errors at the C
# layer ("passing sp_RbVal to parameter of incompatible type
# 'const char *'").
#
# Post-fix: the body-walker harvests usage signals -- here, k/v
# both flowing into str-concat -- and narrows the param to
# str_str_hash (or str_int_hash when v is int-shaped). Both
# compile clean and produce the expected output.
#
# This test re-runs the exact confirmation snippet Ori posted on
# #408; closing the issue without a regression guard would leave
# the narrow path exposed to silent regressions on future
# refactors to the each-body walker.

class T_hash_each_param_narrow_Encoder
  def self.from_str_hash(h)
    out = "{"
    first = true
    h.each do |k, v|
      out += "," unless first
      first = false
      out += "\"" + k + "\":\"" + v + "\""
    end
    out + "}"
  end

  def self.from_int_hash(h)
    out = "{"
    first = true
    h.each do |k, v|
      out += "," unless first
      first = false
      out += "\"" + k + "\":" + v.to_s
    end
    out + "}"
  end
end

puts T_hash_each_param_narrow_Encoder.from_str_hash({"name" => "alice", "city" => "NYC"})
puts T_hash_each_param_narrow_Encoder.from_int_hash({"a" => 1, "b" => 2, "c" => 3})

# === hash_fetch_hash_default ===
# `Hash#fetch(key, {})` on an int-leaf hash. Sequel to #454 which
# closed the string-default case via sp_int_to_s conversion. The
# hash-default case can't unify the two ternary arms to a single
# primitive: get returns int, default is a hash pointer. Box both
# arms to sp_RbVal and surface the return type as poly.

class T_hash_fetch_hash_default_P
  def self.lookup_present(params)
    params.fetch "x", {}
  end

  def self.lookup_missing(params)
    params.fetch "missing", {}
  end
end

box = { "x" => 7 }
v1 = T_hash_fetch_hash_default_P.lookup_present(box)
v2 = T_hash_fetch_hash_default_P.lookup_missing(box)
# Hit path returns the int value (boxed)
puts v1.is_a?(Integer) ? v1 : "non-int"
# Miss path returns the empty hash (boxed)
puts v2.is_a?(Hash) ? "hash" : "non-hash"


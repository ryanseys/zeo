# Bundled tests:
#   - intarray_slice_assign_from_intarray_ptr_array_first
#   - introspect
#   - isa_hash_narrow_typed_param

# === intarray_slice_assign_from_intarray_ptr_array_first ===
# `arr[start, len] = src` slice-assign where `src` came from
# `<X>_ptr_array.first` (or `.last`). Spinel's compile_bracket_assign
# already had a same-prefix slice-assign path for `int_array[i, n] =
# int_array`, but `infer_type` for `.first` / `.last` fell through
# to "int" on a `_ptr_array` receiver, so the slice-assign branch's
# `infer_type(arg_ids[2]) == "int_array"` test missed and the call
# silently lowered to `arr[start] = len` (using arg_ids[1] as the
# value, dropping arg_ids[2] entirely).

class T_intarray_slice_assign_from_intarray_ptr_array_first_C
  def initialize
    buf = (0...32).to_a
    @banks = (0...2).map { buf.slice!(0, 8) }
    @ref = [0] * 24
    @ref[5, 8] = @banks.first
    @ref[15, 8] = @banks.last
  end

  def show
    @ref.each { |x| print x, " " }
    puts
  end
end

T_intarray_slice_assign_from_intarray_ptr_array_first_C.new.show

# === introspect ===
# Test respond_to?, is_a?, class name, to_s

class T_introspect_Animal
  def initialize(name)
    @name = name
  end
  def name; @name; end
  def speak; "..."; end
end

class T_introspect_Dog < T_introspect_Animal
  def speak; "Woof!"; end
  def fetch; "ball"; end
end

d = T_introspect_Dog.new("Rex")

# is_a?
puts d.is_a?(T_introspect_Dog)      # true
puts d.is_a?(T_introspect_Animal)   # true

# respond_to?
puts d.respond_to?(:speak)   # true
puts d.respond_to?(:fetch)   # true
puts d.respond_to?(:fly)     # false

# nil?
puts nil.nil?          # true
puts d.nil?            # false
puts 0.nil?            # false

# Integer predicates
puts 0.zero?           # true
puts 5.positive?       # true
puts (-3).negative?    # true

# === isa_hash_narrow_typed_param ===
# `is_a?(Hash)`-narrowed poly value passed to a typed-Hash param
# slot should unbox via `(sp_<Variant> *).v.p`, matching the
# obj-narrow arm at #448. Pre-fix compile_typed_call_args's
# dispatch only routed `poly`/`string`/array params (and
# obj-typed params when arg was poly) through
# compile_expr_for_expected_type; hash-typed params fell through
# to `compile_expr` and emitted the bare sp_RbVal, failing the
# C compile with "passing 'sp_RbVal' to parameter of incompatible
# type 'sp_StrStrHash *'". Issue #631.

class T_isa_hash_narrow_typed_param_Bag
  def render(hash)
    hash["k"]
  end

  def visit(table)
    seed = { "k" => "v" }
    render(seed)

    val = table["inner"]
    if val.is_a?(Hash)
      render(val)
    else
      "default"
    end
  end
end

b = T_isa_hash_narrow_typed_param_Bag.new
puts b.visit({ "inner" => { "k" => "tokyo" } })


# #582 (Sam Ruby). Function param inferred as poly_array because
# one caller passes a heterogeneous literal `[A.new, "x"]`, but
# another caller passes a homogeneous PtrArray-shaped
# `[A.new, A.new]` (sp_PtrArray of obj_A). The two array layouts
# are incompatible at the C boundary (sp_PtrArray slots are raw
# `void *`, sp_PolyArray slots are tagged sp_RbVal), so the
# call site needs a conversion: allocate a fresh PolyArray and
# box each PtrArray element via sp_box_obj.
#
# Fix: in compile_expr_for_expected_type, detect
# `*_ptr_array → poly_array` at the call boundary and emit an
# inline conversion loop. Element class id comes from the
# ptr_array's elem type (`obj_A` → cls_id for A).

class A
  attr_reader :tag
  def initialize(t = "default"); @tag = t; end
end

def take(arr)
  arr.first
end

a_arr = [A.new("homogeneous"), A.new("entry2")]
mixed = [A.new("mixed_a"), "x"]
r1 = take(a_arr)
r2 = take(mixed)
# r1 should be an A instance (poly value carrying the A boxed via the conversion).
# Verify by reading back the tag via runtime cls_id check.
if r1.is_a?(A)
  puts r1.tag
else
  puts "NOT_A"
end
if r2.is_a?(A)
  puts r2.tag
else
  puts "NOT_A"
end

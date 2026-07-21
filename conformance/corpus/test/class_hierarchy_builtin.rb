# Issue #404 Phase 3 Tier 4. Built-in class prefix in the
# unified cls_id space. Primitives (Integer, String, Array, ...)
# now occupy cls_ids 0..20 per docs/CLASS-OBJECT.md; user
# classes shift to cls_id BC + internal_ci, where BC is the
# reserved built-in count.
#
# Coverage:
#   - Built-in class const in value position: Integer.to_s ->
#     "Integer" via the sp_class_names table.
#   - Built-in class hierarchy: Integer < Numeric, Integer <=
#     Object, Integer < Comparable (transitive via include).
#   - `obj.is_a?(klass)` where obj is a primitive (poly recv via
#     sp_class_for_poly mapping) and klass is a sp_Class value.
#     The pre-Tier-4 path returned the sp_Class{-1} sentinel for
#     primitives.

puts Integer.to_s            # Integer
puts Float.to_s              # Float
puts String.to_s             # String
puts Array.to_s              # Array

# Hierarchy queries on built-in classes.
puts (Integer < Numeric) ? "int<num" : "int!<num"     # int<num
puts (Integer <= Object) ? "int<=obj" : "int!<=obj"   # int<=obj
puts (Integer < Comparable) ? "int<cmp" : "int!<cmp"  # int<cmp (via Numeric)
puts (Float < Numeric) ? "flt<num" : "flt!<num"       # flt<num
puts (String < Object) ? "str<obj" : "str!<obj"       # str<obj

# Dynamic is_a?(klass) where klass is a Class-typed local pointing
# at a built-in. The recv is a poly primitive.
def check(obj, klass)
  obj.is_a?(klass)
end

int_klass = Integer
str_klass = String
num_klass = Numeric

puts check(5, int_klass)     ? "5-int"  : "5-notint"      # 5-int
puts check(5, str_klass)     ? "5-str"  : "5-notstr"      # 5-notstr
puts check(5, num_klass)     ? "5-num"  : "5-notnum"      # 5-num
puts check("hi", str_klass)  ? "hi-str" : "hi-notstr"     # hi-str
puts check("hi", num_klass)  ? "hi-num" : "hi-notnum"     # hi-notnum

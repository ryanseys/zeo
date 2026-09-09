# Built-in classes as values: `Integer.to_s`, and the hierarchy through
# `<` and `<=` against Numeric, Object and Comparable (the last transitive
# through an include). Also `is_a?` where the receiver is a primitive and
# the class is held in a local.

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
__END__
Integer
Float
String
Array
int<num
int<=obj
int<cmp
flt<num
str<obj
5-int
5-notstr
5-num
hi-str
hi-notnum

# A Class object stored in a Hash value or Array element must round-trip
# through poly storage (boxed as a class), not collapse to the int slot
# (which produced an sp_box_int / sp_IntArray_push type mismatch and,
# once stored, printed the cls_id instead of the class name).

str_keyed = { "a" => String, "b" => Integer }
puts str_keyed["a"]
puts str_keyed["b"]

sym_keyed = { foo: String, bar: Array }
puts sym_keyed[:foo]
puts sym_keyed[:bar]

int_keyed = { 1 => String, 2 => Integer }
puts int_keyed[1]
puts int_keyed[2]

classes = [String, Integer, Hash]
puts classes[0]
puts classes[1]
puts classes.length
puts "done"

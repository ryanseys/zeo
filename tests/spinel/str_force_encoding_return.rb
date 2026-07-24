# `def f(s); s.force_encoding(...); end` whose body's last
# expression is `force_encoding` (a receiver passthrough at codegen)
# previously had its return type inferred as mrb_int — the function
# signature returned mrb_int while the body returned a const char *.
#
# Codegen already lowers force_encoding (and encode / b) as a
# receiver passthrough at line 13953; the analyze side just had no
# matching infer_call_type arm, so the call's static type fell back
# to "int". Add force_encoding / encode / b to infer_call_type's
# string-recv arm so the return type carries through.

def normalize(s)
  s.force_encoding("UTF-8")
end

def utf8_to_bin(s)
  s.encode("ASCII-8BIT")
end

def to_binary(s)
  s.b
end

puts normalize("hello")
puts utf8_to_bin("world")
puts to_binary("bytes")

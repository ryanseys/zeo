class C
  @@x = 5
  @@name = "hi"
end
p C.class_variable_get(:@@x)
p C.class_variable_get(:@@name)
p C.class_variable_defined?(:@@x)
p C.class_variable_defined?(:@@nope)
C.class_variable_set(:@@x, 99)
p C.class_variable_get(:@@x)
r = (C.class_variable_get(:@@missing) rescue $!.class); p r

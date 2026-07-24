# class_variable_set with a literal name DECLARES the cvar when the class has
# no such write (CRuby creates it on the fly), and class_variables lists the
# registered ones, own + ancestors (#2719).
class C; @@x = 5; end
C.class_variable_set(:@@y, 9)
p C.class_variable_get(:@@y)
p C.class_variables.sort
p C.class_variable_get(:@@x)
c2 = C
c2.class_variable_set(:@@y, 11)
p C.class_variable_get(:@@y)

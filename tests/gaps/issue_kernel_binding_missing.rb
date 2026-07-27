# Kernel#binding is unimplemented -- zeo raises NameError for the bare
# `binding` call instead of capturing the current scope as a Binding object.
x = 100
b = binding
p b.eval("x")

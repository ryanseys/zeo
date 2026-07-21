# InterpolatedSymbolNode -- `:"foo_#{x}"`.
#
# Lowered to InterpolatedStringNode + sp_intern at codegen side.
# The parser emits InterpolatedSymbolNode with parts; codegen
# treats it like InterpolatedStringNode then routes through
# the intern helper for symbol identity.

x = "world"
sym = :"hello_#{x}"
puts sym                # hello_world

n = 42
sym2 = :"item_#{n}"
puts sym2               # item_42

# Symbol passed through a method
def kind(s) = s.to_s
puts kind(:"prefix_#{x}")   # prefix_world

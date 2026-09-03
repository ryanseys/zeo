# The header mints the class through the runtime; the BODY runs as one
# more `class_eval` of its own source, which is what a class body IS
# -- a scope of its own, with its own cref.

src = "class Minted; def hi; :hi; end; end; Minted.new.hi"
p eval(src)
p Minted.name
__END__
:hi
"Minted"

# `send(name)` where `name` is COMPUTED at run time. A LITERAL
# `send(:block_given?)` folds into the caller exactly like the direct spelling,
# and every literal shape passes in `tests/kernel_scope_intrinsics.rb`. A
# dynamic name resolves to a method ROW, and a row cannot see the caller's block
# or local scope: zeo's `Frame` deliberately carries neither.
#
# So the boundary is precisely "is the method name a compile-time constant", and
# moving it costs more than the divergence does. Carrying the caller's block and
# binding on the `Frame` taxes every call in the program. Folding a `send` whose
# argument is provably one of a small set of literals buys only the shapes that
# were already foldable.
#
# All three intrinsics fail the same way and for the same reason, which is why
# this stays ONE file: `block_given?` needs the caller's block, and `binding`
# and `local_variables` need the caller's local scope.
#
# zeo's answer is a loud `NotImplementedError` naming the row, not a wrong
# value -- which is the part that matters. A caller gets told, rather than told
# something false.
#
# --- ruby 4.0.6 answers ---
# dynamic block_given?: true
# dynamic binding: Binding
# dynamic local_variables: true

# A DECIDED DIVERGENCE -- the golden records ZEO's output and the
# `.divergence` sidecar carries the reason and ruby's answer.
#
# The permanently-hard half of the scope intrinsics: `send(name)` where
# `name` is COMPUTED at runtime. A literal `send(:block_given?)` can be folded
# into the caller at compile time exactly like the direct spelling (that half
# is tracked in `kernel_scope_intrinsics.rb`), but a dynamic name resolves to
# a method row at runtime, and a row cannot see the caller's block or local
# scope -- zeo's `Frame` deliberately carries neither, and widening it would
# tax every call in the program. The ship state is a loud NotImplementedError
# row rather than a wrong answer; ruby, of course, just answers.
#
# The sibling that IS green is `tests/kernel_scope_intrinsics.rb` -- every
# literal spelling, direct and through `send(:literal)`. So the boundary is
# precisely "is the method name a compile-time constant", and a fix would have
# to move it: either by carrying the caller's block and binding on the `Frame`
# (rejected -- it taxes every call), or by folding a `send` whose argument is
# provably one of a small set of literals, which buys only the corpus shapes
# that were already foldable.
#
# All three intrinsics fail the same way and for the same reason, so this
# stays ONE gap: `block_given?` needs the caller's block, `binding` and
# `local_variables` need the caller's local scope, and a method row carries
# neither.

def probe(label)
  puts "#{label}: #{yield.inspect}"
rescue Exception => e # rubocop:disable Lint/RescueException
  puts "#{label}: #{e.class}"
end

name = [:block_given?].sample
def dyn_block(name) = send(name)
probe("dynamic block_given?") { dyn_block(name) { } }

bname = [:binding].sample
probe("dynamic binding") { send(bname).class }

lname = [:local_variables].sample
probe("dynamic local_variables") { q = 1; send(lname).include?(:q) }
__END__
dynamic block_given?: NotImplementedError
dynamic binding: NotImplementedError
dynamic local_variables: NotImplementedError

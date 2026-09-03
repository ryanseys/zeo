# `StringIO.open(string) { |io| ... }` must YIELD the new StringIO to the
# block and answer the block's value, closing the io afterwards -- the same
# contract `File.open` has. zeo's row builds the StringIO and returns it,
# dropping the block entirely, so the answer is the io rather than what the
# block computed.
#
# Found while adding the payload-root class-method probe for value subclasses
# (`B.open` on a `class B < StringIO`); this is the BASE class, so it is not a
# subclassing bug. Fix: the `StringIO.open` row has to call the block with the
# constructed io and return its value.
require "stringio"

p StringIO.open("hi") { |f| f.read }
p StringIO.open("a\nb") { |f| f.readlines }

# Without a block it answers the io, which zeo already does.
p StringIO.open("hi").class

# The io is closed once the block returns.
io = nil
StringIO.open("x") { |f| io = f }
p io.closed?
__END__
"hi"
["a\n", "b"]
StringIO
true

# An IO handle read back out of a poly container.
#
# A stream in a container is boxed, so the receiver's type is only known at run
# time. The poly arm unboxes and dispatches, but its name list covered the
# reader side only -- so `[$stdout, $stderr][0].puts "x"` answered
# `NoMethodError: undefined method 'puts' for an instance of IO`, naming
# exactly what the receiver was.
#
# Names with a non-IO meaning on a poly value stay off that list: `size`, `<<`
# and `each` mean something else on an array and have their own arms.

box = [$stdout, $stderr]

box[0].puts "one"
box[0].print "two\n"
box[0].putc 51
box[0].puts
box[0].puts "four", "five"
box[0].write "six\n"
box[0].flush

p box[0].closed?
p box.length

# a pipe pair through the same container shape: the write end takes the output
# family, the read end the readers
r, w = IO.pipe
pair = [r, w]
pair[1].puts "piped"
pair[1].print "printed\n"
pair[1].close
p pair[0].read
pair[0].close
p pair[0].closed?

# a handle held in a hash value
h = { "out" => $stdout }
h["out"].puts "through a hash"

# a nil-able slot: the union is poly, and a real handle still dispatches
def maybe(f)
  f ? $stdout : nil
end

s = maybe(true)
s.puts "through a union"

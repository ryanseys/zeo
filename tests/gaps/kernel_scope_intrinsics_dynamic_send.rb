# The permanently-hard half of the scope-intrinsics gap: `send(name)` where
# `name` is COMPUTED at runtime. A literal `send(:block_given?)` can be folded
# into the caller at compile time exactly like the direct spelling (that half
# is tracked in `kernel_scope_intrinsics.rb`), but a dynamic name resolves to
# a method row at runtime, and a row cannot see the caller's block or local
# scope -- zeo's `Frame` deliberately carries neither, and widening it would
# tax every call in the program. The ship state is a loud NotImplementedError
# row rather than a wrong answer; ruby, of course, just answers.

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

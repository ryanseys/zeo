# `Ruby::Box.new` in a SNIPPET mints a real box.
#
# `box = Ruby::Box.new` at a program's top level allocates a COMPILE-TIME
# box: the loader mints it, binds the local to its surrogate, and splices
# every `box.require` into the program. A snippet's cannot be one -- the
# handle would name a box nothing else in the program can reach -- so it
# stays the ordinary call, and reaches `Ruby::Box`'s own constructor,
# which mints at run time.
#
# What it used to do instead is why this is written down: the loader
# recognized `b = Ruby::Box.new` in the SNIPPET and minted a compile-time
# box, handing back a surrogate handle to a box the program has no other
# way to name -- and the refusal walk then declined the whole snippet
# with "the source has a `Ruby::Box`", which was true of nothing the
# program had written.

def show(src)
  puts eval([src, "nil"].first).inspect
rescue Exception => e
  puts "#{e.class}: #{e.message}"
end

show("b = Ruby::Box.new; b.class.to_s")
show("Ruby::Box.new.class.to_s")

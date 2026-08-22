# The `Ruby::Box` handle surface, under CRuby's own gate.
#
# Two id spaces are kept apart on purpose. The INTERNAL id is what codegen
# bakes at every site, with `box_id == 0` meaning main at some twenty
# places; the DISPLAY id is CRuby's -- master 1, root 2, main 3, user
# boxes from 4 in creation order -- and is derived, never stored in a key.
#
# `master` and `root` are inert handles: zeo has no boot box, and saying
# so is more honest than faking one. They exist so `Ruby::Box.master` and
# `#master?` answer what ruby answers.
#
# `Ruby::Box.current` cannot be a method row -- a `MethodFn` takes no box,
# so no row can see its caller's. The emitter folds the literal spelling
# to its own site's box, which is why the box's own code answers the box.
b = Ruby::Box.new
c = Ruby::Box.new

# The handle is a real box, and a module besides -- `Ruby::Box < Module`.
p b.class.to_s
p b.is_a?(Ruby::Box)
p Ruby::Box.superclass.to_s
p b.name

# Creation order, from 4.
p [b.inspect, c.inspect]
p [Ruby::Box.main.inspect, Ruby::Box.root.inspect, Ruby::Box.master.inspect]
p [b.main?, b.root?, b.master?]
p [Ruby::Box.main.main?, Ruby::Box.root.root?, Ruby::Box.master.master?]

# `p` renders a handle as the box, not as the module standing in for its
# top level.
p Ruby::Box.current
p Ruby::Box.current.main?

# ... and inside the box, `current` is the box.
p b.eval("Ruby::Box.current.inspect")
p b.eval("Ruby::Box.current.main?")

# A box minted at RUN time is a box like any other.
d = [Ruby::Box.new].first
p [d.class.to_s, d.main?, d.inspect]
d.eval("RUNTIME_BOX_CONST = 7")
p d.eval("RUNTIME_BOX_CONST")
p defined?(RUNTIME_BOX_CONST)

# Each box has its own `$LOAD_PATH`, seeded as a COPY, so an `unshift` in
# one is invisible to the other.
p b.eval("$LOAD_PATH.class.to_s")
b.eval("$LOAD_PATH.unshift('/only-in-the-box')")
p b.eval("$LOAD_PATH.first")
p $LOAD_PATH.include?("/only-in-the-box")

# A definition a box writes on a SHARED class -- one main also sees -- is
# visible from main. Each box gets its own copy of a reopened core class in
# ruby.
#
# This is the narrow remainder of zeo's box isolation, and it is narrow for
# a reason worth stating: a box's own TOP-LEVEL constants and methods are
# already isolated, because `box_top()` routes them onto the box's surrogate
# ClassId and the existing `(owner, name)` key separates boxes. What has no
# box dimension is a write whose OWNER is shared -- `class Array; def
# self.zzz; end; end` inside a box, `Object.const_set` from inside one, a
# class variable on a core class.
#
# The maps that would need the extra key are the overlay's
# `class_methods`/`methods`, `constants`, `cvars` and `civars` -- roughly 115
# call sites of plumbing. The ROADMAP calls this dimension "box-only,
# bundler-irrelevant", and the corpus never hits it, so G7 records it here
# and G8 carries it.
#
# Oracle: main's `Array` never learns the box's class method.
b = Ruby::Box.new
b.eval("class Array; def self.zzz = 9; end")
p Array.respond_to?(:zzz)

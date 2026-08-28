# GAP: `Psych::Nodes::Node#yaml` cannot emit a parse tree back to text.
#
# zeo raises NotImplementedError. Ruby emits the document.
#
# WHY IT IS NOT DONE: zeo's YAML emitter writes a Ruby VALUE. Routing a node
# tree through it would mean `to_ruby` first, which throws away exactly what
# makes a tree worth holding -- the tags, the scalar styles, the anchors and
# the alias placement. A round trip would silently rewrite `!!str 42` as `42`
# and a folded block as a plain scalar. Refusing is the honest answer until a
# node emitter exists; answering wrong text is not.
#
# WHAT IT NEEDS: an emitter that walks `Psych::Nodes::*` rather than a value,
# reusing `ext/psych/ext/psych/src/emitter.rs`'s scalar quoting and line
# folding but taking the style from the node instead of deriving it.
#
# NOTE ruby raises here too, but for an unrelated reason: a bare Document has
# no Stream around it, so libyaml answers `RuntimeError: expected
# STREAM-START`. Wrapping it in a Stream is what a caller does, and that is
# the shape this gap is about.
require "psych"

stream = Psych::Nodes::Stream.new
stream.children << Psych.parse("a: 1\n")
puts stream.yaml

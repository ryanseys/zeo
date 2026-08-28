# GAP: a `Psych::Nodes::*` node does not know where it was written.
#
# `start_line`, `start_column`, `end_line` and `end_column` all answer 0. Ruby
# fills them from the parser's marks.
#
# WHY IT IS NOT DONE YET: the node tree carries what a LOADER needs -- value,
# style, tag, anchor -- and positions are a separate axis that nothing in the
# load path reads. Adding them means widening every `Node` variant with four
# numbers and threading the event marks through, which is worth doing once
# something asks. The parser's marks are available (`nodes.rs` already
# converts one for the block/flow test), so this is bookkeeping rather than
# research.
#
# THE ONE HARD PART, recorded so it is not rediscovered: a container's END is
# real, because its closing event carries its own mark. A SCALAR arrives as
# one event with one mark, so its end has to come from the scanner's token
# span -- the value's own length will not do it, because a folded or literal
# block is de-indented and joined before it reaches the event.
#
# WHY IT MATTERS: an editor or a linter that underlines a node reads the pair,
# and 0:0 underlines the wrong thing rather than nothing.
require "psych"

doc = Psych.parse("a: hello\nb: 2\n")
key = doc.root.children[0]
puts "scalar start #{key.start_line}:#{key.start_column}"
puts "scalar end   #{key.end_line}:#{key.end_column}"
puts "mapping start #{doc.root.start_line}:#{doc.root.start_column}"
puts "mapping end   #{doc.root.end_line}:#{doc.root.end_column}"

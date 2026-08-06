# prism vendored (`gems/prism`). The gem's Ruby half is upstream's, so what
# this guards is the seam: zeo's `Prism::Zeo` module hands `Prism::Serialize`
# the same `pm_serialize_*` buffers upstream's FFI backend does, and the node
# tree, the token stream and the visitor dispatch all come back out of it.
#
# Every answer below is the C extension's too -- the oracle runs that backend
# and this file is blessed from it.
require "prism"

result = Prism.parse("x = 1 + 2")
p result.success?
p result.errors.empty?
node = result.value.statements.body.first
p node.class
p node.name
p node.value.class

# The lexer: token types and their slices, which is what irb's `ruby-lex`
# reads a line's nesting from.
p Prism.lex("foo.bar(1)").value.map { |t,| t.type }

# `parse_lex` answers the tree and the tokens together, from one pass.
tree, tokens = Prism.parse_lex("a = 1\n").value
p tree.class
p tokens.map { |t,| [t.type, t.value] }

# A location carries the line/column/offset pair every consumer indexes by.
loc = Prism.parse("puts 1").value.statements.body.first.location
p [loc.start_line, loc.start_column, loc.start_offset, loc.end_offset]
p loc.slice

# Syntax errors, with and without a tree.
p Prism.parse_success?("1 + 1")
p Prism.parse_success?("1 +")
p Prism.parse_failure?("def")
bad = Prism.parse("def")
p bad.success?
p bad.errors.map { |e| e.message }.first.class

# `scopes:` declares locals the fragment does not bind -- how irb tells a
# bare name from a method call.
p Prism.parse("value", scopes: [[:value]]).value.statements.body.first.class
p Prism.parse("value").value.statements.body.first.class

# A visitor walks the tree, which is how irb colourizes and counts nesting.
class Counter < Prism::Visitor
  attr_reader :calls

  def initialize
    @calls = []
  end

  def visit_call_node(node)
    @calls << node.name
    super
  end
end

counter = Counter.new
Prism.parse("[1, 2].map { |n| n.to_s.upcase }").value.accept(counter)
p counter.calls

# `dump` is the serialized form itself; `Prism.load` is not vendored, but the
# buffer's shape is stable enough to assert it is binary and non-empty.
dumped = Prism.dump("1")
p dumped.encoding
p dumped.bytesize > 0

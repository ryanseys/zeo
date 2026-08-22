# `RubyVM::AbstractSyntaxTree` maps prism's node kinds onto CRuby's NODE_*
# names, and the mapping is INCOMPLETE: an unmapped kind answers a node of
# type `:UNKNOWN` with its location intact and no children, so a walker sees
# a leaf where CRuby has a subtree.
#
# The translator is deliberately a TOTAL function -- an unmapped kind
# degrades to an honest leaf rather than raising, because walkers recurse
# `children` -- and that is the right default. What is missing is the
# mappings themselves: `case`/`when` and `begin`/`rescue` are two of them, and
# they cost 9 of the 13 nodes CRuby reports for the program below.
#
# Each mapped shape is oracle-verified by `tests/rubyvm_ast.rb`, so closing
# this is adding rows in that style, one kind at a time, not a redesign.
#
# NOT part of this gap, and worth keeping apart: `Node#node_id`. zeo numbers
# its own nodes post-order, and CRuby's AST ids are a DIFFERENT scheme from
# prism's own (`nil.nope` is prism id 3 for the call, CRuby AST id 1), so
# neither zeo's numbering nor prism's would serve. See
# `rubyvm_iseq_serialization.rb`.
#
# Oracle: 13 nodes, and no `:UNKNOWN` among them.
src = <<~RB
  case 1
  when Integer then :a
  else :b
  end
  begin
    1
  rescue
    2
  end
RB
def count(n)
  return 0 unless n.is_a?(RubyVM::AbstractSyntaxTree::Node)
  1 + n.children.sum { |c| count(c) }
end
def kinds(n, acc = [])
  return acc unless n.is_a?(RubyVM::AbstractSyntaxTree::Node)
  acc << n.type
  n.children.each { |c| kinds(c, acc) }
  acc
end
ast = RubyVM::AbstractSyntaxTree.parse(src)
p count(ast)
p kinds(ast).uniq.sort

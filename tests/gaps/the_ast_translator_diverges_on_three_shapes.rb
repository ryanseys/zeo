# `RubyVM::AbstractSyntaxTree` maps prism's node kinds onto CRuby's NODE_*
# names, and three shapes are still not the oracle's. None is an
# `:UNKNOWN` -- the kinds are right and the walk is complete; what differs is
# the tree around them.
#
# 1. A `begin` in EXPRESSION position gets a `BEGIN` wrapper in CRuby, and
#    the leading `;` of `begin; 1;` becomes an empty statement inside a
#    BLOCK. prism reports neither, so zeo emits the RESCUE bare.
# 2. `when *y, 3` -- a splat MIXED with ordinary conditions is `ARGSPUSH`,
#    not `LIST`. zeo emits a LIST of both, which is what the pure-splat and
#    pure-list cases already are.
# 3. That `when`'s span ends one column short: CRuby runs it to the `;`,
#    prism stops at the last statement.
#
# Everything measured while closing
# `tests/the_ast_translator_answers_unknown_for_some_shapes.rb` -- the
# case/when, case/in, begin/rescue/ensure and pattern families all agree,
# spans included.
#
# Oracle: the three shapes below.
def dump(n, d = 0)
  return puts("#{'  ' * d}#{n.inspect}") unless n.is_a?(RubyVM::AbstractSyntaxTree::Node)
  puts "#{'  ' * d}#{n.type} #{n.first_lineno},#{n.first_column}-#{n.last_lineno},#{n.last_column}"
  n.children.each { |c| dump(c, d + 1) }
end
dump(RubyVM::AbstractSyntaxTree.parse("x = begin; 1; rescue; 2; end"))
dump(RubyVM::AbstractSyntaxTree.parse("case x; when *y, 3 then 1; end"))

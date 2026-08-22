# `RubyVM::AbstractSyntaxTree` maps prism's node kinds onto CRuby's NODE_*
# names, and the shapes the two disagree about were all one family: a list
# with a SPLAT in it, and the empty statement CRuby's grammar leaves at the
# head of certain bodies.
#
# CRuby builds every element list with `arg_append`/`arg_concat` (parse.y),
# and the accumulator changes SHAPE as the splats land. A run of plain
# elements is a `LIST`; a splat concatenates onto it (`ARGSCAT`, whose body is
# the splatted VALUE, not a `SPLAT` node); a plain element after a splat
# pushes onto that (`ARGSPUSH`); and a SECOND plain element folds the push
# back into an `ARGSCAT` over a two-element `LIST`. zeo emitted a flat `LIST`
# of everything, which was right only when there was no splat -- for a call's
# arguments, an array literal, a `when`'s conditions and a multiple
# assignment alike.
#
# The empty statement is `stmts: none` reducing to `NEW_BEGIN(0)` before the
# first real one, so a body whose text starts with a terminator carries a
# zero-width `BEGIN nil` ahead of it, at the position the statements list
# starts parsing. WHICH terminators survive is a grammar fact, measured
# against the oracle: a `\n` is absorbed everywhere something can take it
# (`def`'s arglist, `while`'s `do`, `if`'s `then`, a `class ... < Super`'s
# term, a block's parameter list) while a `;` never is -- except in a
# `class`/`module` body with NO superclass clause, where nothing is there to
# take either.
#
# Two spans follow the same separator rule: a `when` arm and a `rescue`/
# `ensure` clause run THROUGH a `;` that terminates them, and stop at their
# last statement when there is none.
#
# And a `begin ... end` is a `BEGIN` wrapper in EXPRESSION position and bare
# as a statement, which is the one thing prism's node cannot say for itself.

def dump(n, d = 0)
  return puts("#{'  ' * d}#{n.inspect}") unless n.is_a?(RubyVM::AbstractSyntaxTree::Node)
  puts "#{'  ' * d}#{n.type} #{n.first_lineno},#{n.first_column}-#{n.last_lineno},#{n.last_column}"
  n.children.each { |c| dump(c, d + 1) }
end

[
  # The splat family, in every position that builds an element list.
  "case x; when *y, 3 then 1; end",
  "case x; when *y, 3, 4 then 1; end",
  "case x; when 1, *y then 1; end",
  "case x; when 1, *y, 2 then 1; end",
  "case x; when *y then 1; end",
  "f(*y, 3)",
  "f(1, *y)",
  "[*y, 3]",
  "[1, *y, 2]",
  "a = *y, 3",
  # The empty statement, and the terminators that do and do not survive.
  "x = begin; 1; rescue; 2; end",
  "x = begin; 1; ensure; 2; end",
  "x = begin\n1\nend",
  "x = begin 1 end",
  "begin; 1; rescue; 2; end",
  "class K\n1\nend",
  "class K; 1; end",
  "class K < Object\n1\nend",
  "class K;end",
  "module M\n1\nend",
  "class << self\n1\nend",
  "def m; 1; end",
  "def m; ; 1; end",
  "while x; 1; end",
  "if x; 1; end",
  "[1].each do |z|\n1\nend",
  "[1].each do |z|; 1; end",
  "[1].each {; 1 }",
  "x = (1)",
  "x = (; 1)",
  # The arm and clause spans that run through their separator.
  "case x; when 1 then 2; end",
  "case x; when 1 then 2 end",
  "case x; when 1; end",
  # Node kinds that used to answer :UNKNOWN.
  "A::B",
  "::A::B",
  "A::B = 1",
  "x = __FILE__",
  "x = __LINE__",
  # Interpolated literals, in CRuby's three-child shape.
  '"a#{b}c#{d}e"',
  '"#{b}"',
  '"#{1}#{2}"',
  '"a#@iv"',
  '"a#{ }b"',
  ':"a#{b}"',
  '/a#{b}/',
  '`a#{b}`',
].each do |s|
  puts "### #{s.inspect}"
  dump(RubyVM::AbstractSyntaxTree.parse(s))
end

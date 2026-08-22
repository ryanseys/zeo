# `RubyVM::AbstractSyntaxTree` answers `:UNKNOWN` for an INTERPOLATED literal
# -- a string, symbol, regexp or backtick command with a `#{}` in it, and the
# heredoc forms of each.
#
# The kinds and their shapes are measured. CRuby's node is
# `DSTR [leading_literal, first_EVSTR, rest_or_nil]`, where `rest` is a `LIST`
# of the remaining parts (each an `EVSTR` or a `STR`) and the leading literal
# is a bare Ruby String, `""` when the interpolation comes first. `:"..."` is
# `DSYM`, `/.../` is `DREGX` and a backtick command is `DXSTR`, all three with
# the same three children.
#
# One span rule is not derivable from the parts and has to be copied: the
# FIRST `EVSTR` takes the WHOLE literal's span when nothing follows it
# (`"#{b}"` -> `EVSTR 1,0-1,6`) and its own when something does (`"#{1}#{2}"`
# -> `EVSTR 1,1-1,5`).
#
# The splat-list and empty-statement families are closed -- see
# `tests/the_ast_translator_builds_splat_lists_like_parse_y.rb`, whose header
# carries the `arg_append`/`arg_concat` rules. This is what a differential
# over the translator's own source then turned up, which is the honest
# measure of its coverage: the earlier files sampled shapes, and a real
# program uses interpolation on nearly every line.

def dump(n, d = 0)
  return puts("#{'  ' * d}#{n.inspect}") unless n.is_a?(RubyVM::AbstractSyntaxTree::Node)
  puts "#{'  ' * d}#{n.type} #{n.first_lineno},#{n.first_column}-#{n.last_lineno},#{n.last_column}"
  n.children.each { |c| dump(c, d + 1) }
end

[
  '"a#{b}c"',
  '"#{b}"',
  '"#{1}#{2}"',
  '"a#@iv"',
  ':"a#{b}"',
  '/a#{b}/',
  '`a#{b}`',
].each do |s|
  puts "### #{s.inspect}"
  dump(RubyVM::AbstractSyntaxTree.parse(s))
end

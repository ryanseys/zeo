# Two corners of `RubyVM::AbstractSyntaxTree` over an INTERPOLATED literal.
# The kinds themselves are right now -- `DSTR`/`DSYM`/`DREGX`/`DXSTR` in
# CRuby's three-child shape, `EVSTR` per part, the empty statement inside an
# empty `#{}` -- and so are the spans everywhere but here.
#
# 1. ADJACENT literals are ONE literal in CRuby, and prism nests the second
#    inside the first's parts. The nesting is flattened, but the first
#    `EVSTR` then wants the INNER literal's span (`"a" "#{b}"` -> `EVSTR
#    1,4-1,10`, the whole `"#{b}"`) rather than its own `#{b}`. The rule is
#    the same one a lone `"#{b}"` follows -- a literal that is exactly one
#    interpolation lends it its span -- but which literal that is has to
#    survive the flattening.
#
# 2. A heredoc part that ends AT a newline reports the newline's own line
#    (`2,8`, a column past the line's last character) where zeo rolls onto
#    the next line (`3,0`). That is `Cx::pos`'s convention for an offset
#    sitting exactly on a line start, and CRuby's differs for an END
#    position only -- so changing it means telling the two apart, which no
#    span in this translator currently does.
#
# Everything else in the family agrees, including the translator's own
# source parsed whole.

def dump(n, d = 0)
  return puts("#{'  ' * d}#{n.inspect}") unless n.is_a?(RubyVM::AbstractSyntaxTree::Node)
  puts "#{'  ' * d}#{n.type} #{n.first_lineno},#{n.first_column}-#{n.last_lineno},#{n.last_column}"
  n.children.each { |c| dump(c, d + 1) }
end

[
  '"a" "#{b}"',
  "x = <<~H\n  a\#{b}\nH",
].each do |s|
  puts "### #{s.inspect}"
  dump(RubyVM::AbstractSyntaxTree.parse(s))
end

# `RubyVM::AbstractSyntaxTree` over every prism kind zeo maps, checked shape
# for shape against ruby 4.0.6.
#
# The taxonomy is parse.y's, not prism's, and the two disagree in ways no
# rule derives: `[]` is its own kind, an absent range endpoint is a NIL NODE,
# a `**rest` is a nil KEY, a writer call is an ATTRASGN, `x += 1` desugars
# while `x ||= 1` does not, and `BEGIN { }` is hoisted out of the statement
# list it was written in. Every span below is a measurement.
#
# `tests/rubyvm_ast.rb` covers the Node/Location SURFACE. This file covers the
# node kinds and their children.
$stderr.reopen(IO::NULL)

def dump(n)
  return "nil" if n.nil?
  return n.inspect unless n.is_a?(RubyVM::AbstractSyntaxTree::Node)
  kids = n.children.map { |c| dump(c) }
  "(#{n.type} [#{n.first_lineno}:#{n.first_column}-#{n.last_lineno}:#{n.last_column}] #{kids.join(' ')})"
end

def show(src)
  puts "#{src.inspect} => #{dump(RubyVM::AbstractSyntaxTree.parse(src))}"
rescue SyntaxError
  puts "#{src.inspect} => SyntaxError"
end

puts "-- literals"
["1", "1.5", "1r", "1i", "?a", ":sym", "nil", "true", "false", "self",
 '"s"', "'s'", '"a" "b"', "`ls`", "/re/", "/x/mix", "%r{x}i", "/x/n",
 "%w[a b]", "%i[a b]", "__FILE__", "__LINE__", "__ENCODING__",
 "1..2", "1...2", "(1..)", "(..2)"].each { |s| show(s) }

puts "-- collections"
["[]", "[1,2]", "[*a]", "[1,*a,2]", "[*a,1]", "%w[]",
 "{}", "{a: 1}", "{**h}", "{a: 1, **h}", '{"k" => 1}'].each { |s| show(s) }

puts "-- variables"
["x = 1", "x", "@iv = 1", "@iv", "@@cv = 1", "@@cv", "$gv = 1", "$gv",
 "C = 1", "C", "A::B", "A::B = 1", "::A", "::A::B",
 "$1", "$~", "$&"].each { |s| show(s) }

puts "-- multiple assignment"
["x, y = 1, 2", "x, *y = 1, 2", "*x, y = 1", "a, (b, c) = 1", "a, = 1",
 "a, *, b = 1", "*, a = 1", "@a, @b = 1", "a.b, c[0] = 1",
 "A, B = 1", "x = y = 1"].each { |s| show(s) }

puts "-- operator assignment"
["x += 1", "x ||= 1", "x &&= 1", "@iv += 1", "@@c += 1", "$gv ||= 1",
 "C ||= 1", "a[0] += 1", "a[0] ||= 1", "a.b += 1", "a&.b += 1",
 "A::B ||= 1", "A::B += 1"].each { |s| show(s) }

puts "-- calls"
["foo", "foo()", "foo 1", "foo(1, 2)", "a.b", "a.b(1)", "a&.b", "a&.b(1)",
 "a[1]", "a[1, 2]", "a[1] = 2", "a.b = 1", "a&.b = 1",
 "foo(*a)", "foo(**h)", "foo(&b)", "foo(1, &b)", "foo(a: 1)", "foo(&:sym)",
 "1 + 2", "-a", "~a", "a <=> b", "a =~ b", "a !~ b", "!a", "-2**2",
 "foo { }", "foo { |x| x }", "foo(1) { }", "a.b { }", "foo { |x, (y, z)| x }",
 "foo { |x, *y, **z, &w| x }", "foo { _1 }", "foo { it }",
 "yield", "yield 1", "super", "super()", "super 1", "super { }",
 "->(x) { x }", "->{ }", "defined?(a)"].each { |s| show(s) }

puts "-- definitions"
["def m; end", "def m(); end", "def m(a); end", "def m(a = 1); end",
 "def m(*a); end", "def m(*); end", "def m(a:); end", "def m(a: 1); end",
 "def m(**k); end", "def m(**); end", "def m(**nil); end", "def m(&b); end",
 "def m(&); end", "def m(a, *b, c); end", "def m(a, (b, c)); end",
 "def m(a, b = 1, *c, d, e:, f: 2, **g, &h); end",
 "def self.m; end", "def m = 1", "def m; 1; end", "def m(); 1; end",
 "def m; rescue; end", "def m(a, ...); n(...); end", "def m(...) = n(...)",
 "class Foo; end", "class Foo < Bar; end", "class Foo::Bar; end",
 "class << self; end", "module M; end", "module M::N; end",
 "alias a b", "alias $a $b", "undef a", "undef a, b"].each { |s| show(s) }

puts "-- control flow"
["if a then 1 else 2 end", "if a; end", "if a; elsif b; else; end",
 "unless a; else; end", "1 if a", "a ? 1 : 2",
 "while a; end", "while a do b end", "until a; end",
 "begin; a; end while b", "for i in a; end", "for a, b in c; end",
 "case a; when 1 then 2; else 3; end", "case a; when *b then 1; end",
 "break", "break 1", "next", "redo", "retry", "return 1",
 "begin; a; rescue => e; b; else; c; ensure; d; end",
 "begin; a; rescue A, B => e; end", "a rescue b",
 "a and b", "a or b", "not a", "a && b", "a || b"].each { |s| show(s) }

puts "-- pattern matching"
["case a; in 1 then 2; end", "case a; in [x, y] then 1; end",
 "case a; in {k: v} then 1; end", "case a; in Integer => n then n; end",
 "case a; in 1 | 2 then 3; end", "case a; in x if x > 1 then 1; end",
 "case a; in []; end", "case a; in [*]; end", "case a; in {}; end",
 "case a; in **nil; end", "case a; in ^b; end", "case a; in ^(1+1); end",
 "case a; in [1, *r, 2]; end", "case a; in {a:, **r}; end",
 "case a; in Foo(x:); end", "case a; in Foo[x]; end",
 "a => b", "a in b"].each { |s| show(s) }

puts "-- hoisting and empty programs"
["BEGIN { 1 }", "BEGIN { }", "1; BEGIN { 2 }", "BEGIN { 1 }; BEGIN { 2 }",
 "END { 1 }", "END { }", "1; END { 2 }",
 "()", "(a)", "(a; b)", ";", "a; b", "a ; # comment", "",
 "# only a comment\n"].each { |s| show(s) }

puts "-- multiline"
show("if a\n  b\nelsif c\n  d\nelse\n  e\nend")
show("class A\n  def b\n    1\n  end\nend")
show("begin\n  a\nrescue X => e\n  b\nelse\n  c\nensure\n  d\nend")
show("case a\nin {a:, **r}\n  1\nin String => s if s\n  2\nelse\n  3\nend")
show("x = <<~E\n  a\nE\n")

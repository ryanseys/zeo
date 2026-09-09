# `RubyVM::AbstractSyntaxTree` over every prism kind zeo maps, checked shape
# for shape against ruby 4.0.6.
#
# The taxonomy is parse.y's, not prism's, and the two disagree in ways no
# rule derives: `[]` is its own kind, an absent range endpoint is a NIL NODE,
# a `**rest` is a nil KEY, a writer call is an ATTRASGN, `x += 1` desugars
# while `x ||= 1` does not, and `BEGIN { }` is hoisted out of the statement
# list it was written in. Every span below is a measurement.
#
# `test/core/rubyvm/rubyvm_ast.rb` covers the Node/Location SURFACE. This file covers the
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
__END__
-- literals
"1" => (SCOPE [1:0-1:1] [] nil (INTEGER [1:0-1:1] 1))
"1.5" => (SCOPE [1:0-1:3] [] nil (FLOAT [1:0-1:3] 1.5))
"1r" => (SCOPE [1:0-1:2] [] nil (RATIONAL [1:0-1:2] (1/1)))
"1i" => (SCOPE [1:0-1:2] [] nil (IMAGINARY [1:0-1:2] (0+1i)))
"?a" => (SCOPE [1:0-1:2] [] nil (STR [1:0-1:2] "a"))
":sym" => (SCOPE [1:0-1:4] [] nil (SYM [1:0-1:4] :sym))
"nil" => (SCOPE [1:0-1:3] [] nil (NIL [1:0-1:3] ))
"true" => (SCOPE [1:0-1:4] [] nil (TRUE [1:0-1:4] ))
"false" => (SCOPE [1:0-1:5] [] nil (FALSE [1:0-1:5] ))
"self" => (SCOPE [1:0-1:4] [] nil (SELF [1:0-1:4] ))
"\"s\"" => (SCOPE [1:0-1:3] [] nil (STR [1:0-1:3] "s"))
"'s'" => (SCOPE [1:0-1:3] [] nil (STR [1:0-1:3] "s"))
"\"a\" \"b\"" => (SCOPE [1:0-1:7] [] nil (STR [1:0-1:3] "ab"))
"`ls`" => (SCOPE [1:0-1:4] [] nil (XSTR [1:0-1:4] "ls"))
"/re/" => (SCOPE [1:0-1:4] [] nil (REGX [1:0-1:4] /re/))
"/x/mix" => (SCOPE [1:0-1:6] [] nil (REGX [1:0-1:6] /x/mix))
"%r{x}i" => (SCOPE [1:0-1:6] [] nil (REGX [1:0-1:6] /x/i))
"/x/n" => (SCOPE [1:0-1:4] [] nil (REGX [1:0-1:4] /x/n))
"%w[a b]" => (SCOPE [1:0-1:7] [] nil (LIST [1:0-1:7] (STR [1:3-1:4] "a") (STR [1:5-1:6] "b") nil))
"%i[a b]" => (SCOPE [1:0-1:7] [] nil (LIST [1:0-1:7] (SYM [1:3-1:4] :a) (SYM [1:5-1:6] :b) nil))
"__FILE__" => (SCOPE [1:0-1:8] [] nil (FILE [1:0-1:8] ""))
"__LINE__" => (SCOPE [1:0-1:8] [] nil (LINE [1:0-1:8] 1))
"__ENCODING__" => (SCOPE [1:0-1:12] [] nil (ENCODING [1:0-1:12] #<Encoding:UTF-8>))
"1..2" => (SCOPE [1:0-1:4] [] nil (DOT2 [1:0-1:4] (INTEGER [1:0-1:1] 1) (INTEGER [1:3-1:4] 2)))
"1...2" => (SCOPE [1:0-1:5] [] nil (DOT3 [1:0-1:5] (INTEGER [1:0-1:1] 1) (INTEGER [1:4-1:5] 2)))
"(1..)" => (SCOPE [1:0-1:5] [] nil (BLOCK [1:0-1:5] (DOT2 [1:1-1:4] (INTEGER [1:1-1:2] 1) (NIL [1:4-1:4] ))))
"(..2)" => (SCOPE [1:0-1:5] [] nil (BLOCK [1:0-1:5] (DOT2 [1:1-1:4] (NIL [1:1-1:1] ) (INTEGER [1:3-1:4] 2))))
-- collections
"[]" => (SCOPE [1:0-1:2] [] nil (ZLIST [1:0-1:2] ))
"[1,2]" => (SCOPE [1:0-1:5] [] nil (LIST [1:0-1:5] (INTEGER [1:1-1:2] 1) (INTEGER [1:3-1:4] 2) nil))
"[*a]" => (SCOPE [1:0-1:4] [] nil (SPLAT [1:0-1:4] (VCALL [1:2-1:3] :a)))
"[1,*a,2]" => (SCOPE [1:0-1:8] [] nil (ARGSPUSH [1:0-1:8] (ARGSCAT [1:1-1:5] (LIST [1:1-1:2] (INTEGER [1:1-1:2] 1) nil) (VCALL [1:4-1:5] :a)) (INTEGER [1:6-1:7] 2)))
"[*a,1]" => (SCOPE [1:0-1:6] [] nil (ARGSPUSH [1:0-1:6] (SPLAT [1:1-1:3] (VCALL [1:2-1:3] :a)) (INTEGER [1:4-1:5] 1)))
"%w[]" => (SCOPE [1:0-1:4] [] nil (ZLIST [1:0-1:4] ))
"{}" => (SCOPE [1:0-1:2] [] nil (HASH [1:0-1:2] nil))
"{a: 1}" => (SCOPE [1:0-1:6] [] nil (HASH [1:0-1:6] (LIST [1:1-1:5] (SYM [1:1-1:3] :a) (INTEGER [1:4-1:5] 1) nil)))
"{**h}" => (SCOPE [1:0-1:5] [] nil (HASH [1:0-1:5] (LIST [1:1-1:4] nil (VCALL [1:3-1:4] :h) nil)))
"{a: 1, **h}" => (SCOPE [1:0-1:11] [] nil (HASH [1:0-1:11] (LIST [1:1-1:10] (SYM [1:1-1:3] :a) (INTEGER [1:4-1:5] 1) nil (VCALL [1:9-1:10] :h) nil)))
"{\"k\" => 1}" => (SCOPE [1:0-1:10] [] nil (HASH [1:0-1:10] (LIST [1:1-1:9] (STR [1:1-1:4] "k") (INTEGER [1:8-1:9] 1) nil)))
-- variables
"x = 1" => (SCOPE [1:0-1:5] [:x] nil (LASGN [1:0-1:5] :x (INTEGER [1:4-1:5] 1)))
"x" => (SCOPE [1:0-1:1] [] nil (VCALL [1:0-1:1] :x))
"@iv = 1" => (SCOPE [1:0-1:7] [] nil (IASGN [1:0-1:7] :@iv (INTEGER [1:6-1:7] 1)))
"@iv" => (SCOPE [1:0-1:3] [] nil (IVAR [1:0-1:3] :@iv))
"@@cv = 1" => (SCOPE [1:0-1:8] [] nil (CVASGN [1:0-1:8] :@@cv (INTEGER [1:7-1:8] 1)))
"@@cv" => (SCOPE [1:0-1:4] [] nil (CVAR [1:0-1:4] :@@cv))
"$gv = 1" => (SCOPE [1:0-1:7] [] nil (GASGN [1:0-1:7] :$gv (INTEGER [1:6-1:7] 1)))
"$gv" => (SCOPE [1:0-1:3] [] nil (GVAR [1:0-1:3] :$gv))
"C = 1" => (SCOPE [1:0-1:5] [] nil (CDECL [1:0-1:5] :C (INTEGER [1:4-1:5] 1)))
"C" => (SCOPE [1:0-1:1] [] nil (CONST [1:0-1:1] :C))
"A::B" => (SCOPE [1:0-1:4] [] nil (COLON2 [1:0-1:4] (CONST [1:0-1:1] :A) :B))
"A::B = 1" => (SCOPE [1:0-1:8] [] nil (CDECL [1:0-1:8] (COLON2 [1:0-1:4] (CONST [1:0-1:1] :A) :B) :B (INTEGER [1:7-1:8] 1)))
"::A" => (SCOPE [1:0-1:3] [] nil (COLON3 [1:0-1:3] :A))
"::A::B" => (SCOPE [1:0-1:6] [] nil (COLON2 [1:0-1:6] (COLON3 [1:0-1:3] :A) :B))
"$1" => (SCOPE [1:0-1:2] [] nil (NTH_REF [1:0-1:2] :$1))
"$~" => (SCOPE [1:0-1:2] [] nil (GVAR [1:0-1:2] :$~))
"$&" => (SCOPE [1:0-1:2] [] nil (BACK_REF [1:0-1:2] :$&))
-- multiple assignment
"x, y = 1, 2" => (SCOPE [1:0-1:11] [:x, :y] nil (MASGN [1:0-1:11] (LIST [1:7-1:11] (INTEGER [1:7-1:8] 1) (INTEGER [1:10-1:11] 2) nil) (LIST [1:0-1:4] (LASGN [1:0-1:1] :x nil) (LASGN [1:3-1:4] :y nil) nil) nil))
"x, *y = 1, 2" => (SCOPE [1:0-1:12] [:x, :y] nil (MASGN [1:0-1:12] (LIST [1:8-1:12] (INTEGER [1:8-1:9] 1) (INTEGER [1:11-1:12] 2) nil) (LIST [1:0-1:1] (LASGN [1:0-1:1] :x nil) nil) (LASGN [1:4-1:5] :y nil)))
"*x, y = 1" => (SCOPE [1:0-1:9] [:x, :y] nil (MASGN [1:0-1:9] (INTEGER [1:8-1:9] 1) nil (POSTARG [1:0-1:5] (LASGN [1:1-1:2] :x nil) (LIST [1:4-1:5] (LASGN [1:4-1:5] :y nil) nil))))
"a, (b, c) = 1" => (SCOPE [1:0-1:13] [:a, :b, :c] nil (MASGN [1:0-1:13] (INTEGER [1:12-1:13] 1) (LIST [1:0-1:8] (LASGN [1:0-1:1] :a nil) (MASGN [1:4-1:8] nil (LIST [1:4-1:8] (LASGN [1:4-1:5] :b nil) (LASGN [1:7-1:8] :c nil) nil) nil) nil) nil))
"a, = 1" => (SCOPE [1:0-1:6] [:a] nil (MASGN [1:0-1:6] (INTEGER [1:5-1:6] 1) (LIST [1:0-1:1] (LASGN [1:0-1:1] :a nil) nil) nil))
"a, *, b = 1" => (SCOPE [1:0-1:11] [:a, :b] nil (MASGN [1:0-1:11] (INTEGER [1:10-1:11] 1) (LIST [1:0-1:1] (LASGN [1:0-1:1] :a nil) nil) (POSTARG [1:0-1:7] :NODE_SPECIAL_NO_NAME_REST (LIST [1:6-1:7] (LASGN [1:6-1:7] :b nil) nil))))
"*, a = 1" => (SCOPE [1:0-1:8] [:a] nil (MASGN [1:0-1:8] (INTEGER [1:7-1:8] 1) nil (POSTARG [1:0-1:4] :NODE_SPECIAL_NO_NAME_REST (LIST [1:3-1:4] (LASGN [1:3-1:4] :a nil) nil))))
"@a, @b = 1" => (SCOPE [1:0-1:10] [] nil (MASGN [1:0-1:10] (INTEGER [1:9-1:10] 1) (LIST [1:0-1:6] (IASGN [1:0-1:2] :@a nil) (IASGN [1:4-1:6] :@b nil) nil) nil))
"a.b, c[0] = 1" => (SCOPE [1:0-1:13] [] nil (MASGN [1:0-1:13] (INTEGER [1:12-1:13] 1) (LIST [1:0-1:9] (ATTRASGN [1:0-1:3] (VCALL [1:0-1:1] :a) :b= nil) (ATTRASGN [1:5-1:9] (VCALL [1:5-1:6] :c) :[]= (LIST [1:7-1:8] (INTEGER [1:7-1:8] 0) nil)) nil) nil))
"A, B = 1" => (SCOPE [1:0-1:8] [] nil (MASGN [1:0-1:8] (INTEGER [1:7-1:8] 1) (LIST [1:0-1:4] (CDECL [1:0-1:1] :A nil) (CDECL [1:3-1:4] :B nil) nil) nil))
"x = y = 1" => (SCOPE [1:0-1:9] [:x, :y] nil (LASGN [1:0-1:9] :x (LASGN [1:4-1:9] :y (INTEGER [1:8-1:9] 1))))
-- operator assignment
"x += 1" => (SCOPE [1:0-1:6] [:x] nil (LASGN [1:0-1:6] :x (CALL [1:0-1:6] (LVAR [1:0-1:1] :x) :+ (LIST [1:5-1:6] (INTEGER [1:5-1:6] 1) nil))))
"x ||= 1" => (SCOPE [1:0-1:7] [:x] nil (OP_ASGN_OR [1:0-1:7] (LVAR [1:0-1:1] :x) :"||" (LASGN [1:0-1:7] :x (INTEGER [1:6-1:7] 1))))
"x &&= 1" => (SCOPE [1:0-1:7] [:x] nil (OP_ASGN_AND [1:0-1:7] (LVAR [1:0-1:1] :x) :"&&" (LASGN [1:0-1:7] :x (INTEGER [1:6-1:7] 1))))
"@iv += 1" => (SCOPE [1:0-1:8] [] nil (IASGN [1:0-1:8] :@iv (CALL [1:0-1:8] (IVAR [1:0-1:3] :@iv) :+ (LIST [1:7-1:8] (INTEGER [1:7-1:8] 1) nil))))
"@@c += 1" => (SCOPE [1:0-1:8] [] nil (CVASGN [1:0-1:8] :@@c (CALL [1:0-1:8] (CVAR [1:0-1:3] :@@c) :+ (LIST [1:7-1:8] (INTEGER [1:7-1:8] 1) nil))))
"$gv ||= 1" => (SCOPE [1:0-1:9] [] nil (OP_ASGN_OR [1:0-1:9] (GVAR [1:0-1:3] :$gv) :"||" (GASGN [1:0-1:9] :$gv (INTEGER [1:8-1:9] 1))))
"C ||= 1" => (SCOPE [1:0-1:7] [] nil (OP_ASGN_OR [1:0-1:7] (CONST [1:0-1:1] :C) :"||" (CDECL [1:0-1:7] :C (INTEGER [1:6-1:7] 1))))
"a[0] += 1" => (SCOPE [1:0-1:9] [] nil (OP_ASGN1 [1:0-1:9] (VCALL [1:0-1:1] :a) :+ (LIST [1:2-1:3] (INTEGER [1:2-1:3] 0) nil) (INTEGER [1:8-1:9] 1)))
"a[0] ||= 1" => (SCOPE [1:0-1:10] [] nil (OP_ASGN1 [1:0-1:10] (VCALL [1:0-1:1] :a) :"||" (LIST [1:2-1:3] (INTEGER [1:2-1:3] 0) nil) (INTEGER [1:9-1:10] 1)))
"a.b += 1" => (SCOPE [1:0-1:8] [] nil (OP_ASGN2 [1:0-1:8] (VCALL [1:0-1:1] :a) false :b :+ (INTEGER [1:7-1:8] 1)))
"a&.b += 1" => (SCOPE [1:0-1:9] [] nil (OP_ASGN2 [1:0-1:9] (VCALL [1:0-1:1] :a) true :b :+ (INTEGER [1:8-1:9] 1)))
"A::B ||= 1" => (SCOPE [1:0-1:10] [] nil (OP_CDECL [1:0-1:10] (COLON2 [1:0-1:4] (CONST [1:0-1:1] :A) :B) :"||" (INTEGER [1:9-1:10] 1)))
"A::B += 1" => (SCOPE [1:0-1:9] [] nil (OP_CDECL [1:0-1:9] (COLON2 [1:0-1:4] (CONST [1:0-1:1] :A) :B) :+ (INTEGER [1:8-1:9] 1)))
-- calls
"foo" => (SCOPE [1:0-1:3] [] nil (VCALL [1:0-1:3] :foo))
"foo()" => (SCOPE [1:0-1:5] [] nil (FCALL [1:0-1:5] :foo nil))
"foo 1" => (SCOPE [1:0-1:5] [] nil (FCALL [1:0-1:5] :foo (LIST [1:4-1:5] (INTEGER [1:4-1:5] 1) nil)))
"foo(1, 2)" => (SCOPE [1:0-1:9] [] nil (FCALL [1:0-1:9] :foo (LIST [1:4-1:8] (INTEGER [1:4-1:5] 1) (INTEGER [1:7-1:8] 2) nil)))
"a.b" => (SCOPE [1:0-1:3] [] nil (CALL [1:0-1:3] (VCALL [1:0-1:1] :a) :b nil))
"a.b(1)" => (SCOPE [1:0-1:6] [] nil (CALL [1:0-1:6] (VCALL [1:0-1:1] :a) :b (LIST [1:4-1:5] (INTEGER [1:4-1:5] 1) nil)))
"a&.b" => (SCOPE [1:0-1:4] [] nil (QCALL [1:0-1:4] (VCALL [1:0-1:1] :a) :b nil))
"a&.b(1)" => (SCOPE [1:0-1:7] [] nil (QCALL [1:0-1:7] (VCALL [1:0-1:1] :a) :b (LIST [1:5-1:6] (INTEGER [1:5-1:6] 1) nil)))
"a[1]" => (SCOPE [1:0-1:4] [] nil (CALL [1:0-1:4] (VCALL [1:0-1:1] :a) :[] (LIST [1:2-1:3] (INTEGER [1:2-1:3] 1) nil)))
"a[1, 2]" => (SCOPE [1:0-1:7] [] nil (CALL [1:0-1:7] (VCALL [1:0-1:1] :a) :[] (LIST [1:2-1:6] (INTEGER [1:2-1:3] 1) (INTEGER [1:5-1:6] 2) nil)))
"a[1] = 2" => (SCOPE [1:0-1:8] [] nil (ATTRASGN [1:0-1:8] (VCALL [1:0-1:1] :a) :[]= (LIST [1:2-1:8] (INTEGER [1:2-1:3] 1) (INTEGER [1:7-1:8] 2) nil)))
"a.b = 1" => (SCOPE [1:0-1:7] [] nil (ATTRASGN [1:0-1:7] (VCALL [1:0-1:1] :a) :b= (LIST [1:6-1:7] (INTEGER [1:6-1:7] 1) nil)))
"a&.b = 1" => (SCOPE [1:0-1:8] [] nil (ATTRASGN [1:0-1:8] (VCALL [1:0-1:1] :a) :b (LIST [1:7-1:8] (INTEGER [1:7-1:8] 1) nil)))
"foo(*a)" => (SCOPE [1:0-1:7] [] nil (FCALL [1:0-1:7] :foo (SPLAT [1:4-1:6] (VCALL [1:5-1:6] :a))))
"foo(**h)" => (SCOPE [1:0-1:8] [] nil (FCALL [1:0-1:8] :foo (LIST [1:4-1:7] (HASH [1:4-1:7] (LIST [1:4-1:7] nil (VCALL [1:6-1:7] :h) nil)) nil)))
"foo(&b)" => (SCOPE [1:0-1:7] [] nil (FCALL [1:0-1:7] :foo (BLOCK_PASS [1:4-1:6] nil (VCALL [1:5-1:6] :b))))
"foo(1, &b)" => (SCOPE [1:0-1:10] [] nil (FCALL [1:0-1:10] :foo (BLOCK_PASS [1:4-1:9] (LIST [1:4-1:5] (INTEGER [1:4-1:5] 1) nil) (VCALL [1:8-1:9] :b))))
"foo(a: 1)" => (SCOPE [1:0-1:9] [] nil (FCALL [1:0-1:9] :foo (LIST [1:4-1:8] (HASH [1:4-1:8] (LIST [1:4-1:8] (SYM [1:4-1:6] :a) (INTEGER [1:7-1:8] 1) nil)) nil)))
"foo(&:sym)" => (SCOPE [1:0-1:10] [] nil (FCALL [1:0-1:10] :foo (BLOCK_PASS [1:4-1:9] nil (SYM [1:5-1:9] :sym))))
"1 + 2" => (SCOPE [1:0-1:5] [] nil (OPCALL [1:0-1:5] (INTEGER [1:0-1:1] 1) :+ (LIST [1:4-1:5] (INTEGER [1:4-1:5] 2) nil)))
"-a" => (SCOPE [1:0-1:2] [] nil (OPCALL [1:0-1:2] (VCALL [1:1-1:2] :a) :-@ nil))
"~a" => (SCOPE [1:0-1:2] [] nil (OPCALL [1:0-1:2] (VCALL [1:1-1:2] :a) :~ nil))
"a <=> b" => (SCOPE [1:0-1:7] [] nil (OPCALL [1:0-1:7] (VCALL [1:0-1:1] :a) :<=> (LIST [1:6-1:7] (VCALL [1:6-1:7] :b) nil)))
"a =~ b" => (SCOPE [1:0-1:6] [] nil (CALL [1:0-1:6] (VCALL [1:0-1:1] :a) :=~ (LIST [1:5-1:6] (VCALL [1:5-1:6] :b) nil)))
"a !~ b" => (SCOPE [1:0-1:6] [] nil (OPCALL [1:0-1:6] (VCALL [1:0-1:1] :a) :!~ (LIST [1:5-1:6] (VCALL [1:5-1:6] :b) nil)))
"!a" => (SCOPE [1:0-1:2] [] nil (OPCALL [1:0-1:2] (VCALL [1:1-1:2] :a) :! nil))
"-2**2" => (SCOPE [1:0-1:5] [] nil (OPCALL [1:0-1:5] (OPCALL [1:0-1:5] (INTEGER [1:1-1:2] 2) :** (LIST [1:4-1:5] (INTEGER [1:4-1:5] 2) nil)) :-@ nil))
"foo { }" => (SCOPE [1:0-1:7] [] nil (ITER [1:0-1:7] (FCALL [1:0-1:3] :foo nil) (SCOPE [1:4-1:7] [] nil (BEGIN [1:5-1:5] nil))))
"foo { |x| x }" => (SCOPE [1:0-1:13] [] nil (ITER [1:0-1:13] (FCALL [1:0-1:3] :foo nil) (SCOPE [1:4-1:13] [:x] (ARGS [1:7-1:8] 1 nil nil nil 0 nil nil nil nil nil) (DVAR [1:10-1:11] :x))))
"foo(1) { }" => (SCOPE [1:0-1:10] [] nil (ITER [1:0-1:10] (FCALL [1:0-1:6] :foo (LIST [1:4-1:5] (INTEGER [1:4-1:5] 1) nil)) (SCOPE [1:7-1:10] [] nil (BEGIN [1:8-1:8] nil))))
"a.b { }" => (SCOPE [1:0-1:7] [] nil (ITER [1:0-1:7] (CALL [1:0-1:3] (VCALL [1:0-1:1] :a) :b nil) (SCOPE [1:4-1:7] [] nil (BEGIN [1:5-1:5] nil))))
"foo { |x, (y, z)| x }" => (SCOPE [1:0-1:21] [] nil (ITER [1:0-1:21] (FCALL [1:0-1:3] :foo nil) (SCOPE [1:4-1:21] [:x, nil, :y, :z] (ARGS [1:7-1:16] 2 (MASGN [1:11-1:15] (DVAR [1:11-1:11] nil) (LIST [1:11-1:15] (DASGN [1:11-1:12] :y nil) (DASGN [1:14-1:15] :z nil) nil) nil) nil nil 0 nil nil nil nil nil) (DVAR [1:18-1:19] :x))))
"foo { |x, *y, **z, &w| x }" => (SCOPE [1:0-1:26] [] nil (ITER [1:0-1:26] (FCALL [1:0-1:3] :foo nil) (SCOPE [1:4-1:26] [:x, :y, :z, :w] (ARGS [1:7-1:21] 1 nil nil nil 0 nil :y nil (DVAR [1:14-1:17] :z) :w) (DVAR [1:23-1:24] :x))))
"foo { _1 }" => (SCOPE [1:0-1:10] [] nil (ITER [1:0-1:10] (FCALL [1:0-1:3] :foo nil) (SCOPE [1:4-1:10] [:_1] (ARGS [1:10-1:10] 1 nil nil nil 0 nil nil nil nil nil) (DVAR [1:6-1:8] :_1))))
"foo { it }" => (SCOPE [1:0-1:10] [] nil (ITER [1:0-1:10] (FCALL [1:0-1:3] :foo nil) (SCOPE [1:4-1:10] [:"<it>"] (ARGS [1:10-1:10] 1 nil nil nil 0 nil nil nil nil nil) (DVAR [1:6-1:8] :"<it>"))))
"yield" => SyntaxError
"yield 1" => SyntaxError
"super" => (SCOPE [1:0-1:5] [] nil (ZSUPER [1:0-1:5] ))
"super()" => (SCOPE [1:0-1:7] [] nil (SUPER [1:0-1:7] nil))
"super 1" => (SCOPE [1:0-1:7] [] nil (SUPER [1:0-1:7] (LIST [1:6-1:7] (INTEGER [1:6-1:7] 1) nil)))
"super { }" => (SCOPE [1:0-1:9] [] nil (ITER [1:0-1:9] (ZSUPER [1:0-1:5] ) (SCOPE [1:6-1:9] [] nil (BEGIN [1:7-1:7] nil))))
"->(x) { x }" => (SCOPE [1:0-1:11] [] nil (LAMBDA [1:0-1:11] (SCOPE [1:2-1:11] [:x] (ARGS [1:3-1:4] 1 nil nil nil 0 nil nil nil nil nil) (DVAR [1:8-1:9] :x))))
"->{ }" => (SCOPE [1:0-1:5] [] nil (LAMBDA [1:0-1:5] (SCOPE [1:2-1:5] [] (ARGS [1:2-1:2] 0 nil nil nil 0 nil nil nil nil nil) (BEGIN [1:3-1:3] nil))))
"defined?(a)" => (SCOPE [1:0-1:11] [] nil (DEFINED [1:0-1:11] (VCALL [1:9-1:10] :a)))
-- definitions
"def m; end" => (SCOPE [1:0-1:10] [] nil (DEFN [1:0-1:10] :m (SCOPE [1:0-1:10] [] (ARGS [1:5-1:5] 0 nil nil nil 0 nil nil nil nil nil) nil)))
"def m(); end" => (SCOPE [1:0-1:12] [] nil (DEFN [1:0-1:12] :m (SCOPE [1:0-1:12] [] (ARGS [1:5-1:6] 0 nil nil nil 0 nil nil nil nil nil) nil)))
"def m(a); end" => (SCOPE [1:0-1:13] [] nil (DEFN [1:0-1:13] :m (SCOPE [1:0-1:13] [:a] (ARGS [1:6-1:7] 1 nil nil nil 0 nil nil nil nil nil) nil)))
"def m(a = 1); end" => (SCOPE [1:0-1:17] [] nil (DEFN [1:0-1:17] :m (SCOPE [1:0-1:17] [:a] (ARGS [1:6-1:11] 0 nil (OPT_ARG [1:6-1:11] (LASGN [1:6-1:11] :a (INTEGER [1:10-1:11] 1)) nil) nil 0 nil nil nil nil nil) nil)))
"def m(*a); end" => (SCOPE [1:0-1:14] [] nil (DEFN [1:0-1:14] :m (SCOPE [1:0-1:14] [:a] (ARGS [1:6-1:8] 0 nil nil nil 0 nil :a nil nil nil) nil)))
"def m(*); end" => (SCOPE [1:0-1:13] [] nil (DEFN [1:0-1:13] :m (SCOPE [1:0-1:13] [:*] (ARGS [1:6-1:7] 0 nil nil nil 0 nil :* nil nil nil) nil)))
"def m(a:); end" => (SCOPE [1:0-1:14] [] nil (DEFN [1:0-1:14] :m (SCOPE [1:0-1:14] [:a, nil] (ARGS [1:6-1:8] 0 nil nil nil 0 nil nil (KW_ARG [1:6-1:8] (LASGN [1:6-1:8] :a :NODE_SPECIAL_REQUIRED_KEYWORD) nil) (DVAR [1:6-1:8] nil) nil) nil)))
"def m(a: 1); end" => (SCOPE [1:0-1:16] [] nil (DEFN [1:0-1:16] :m (SCOPE [1:0-1:16] [:a, nil] (ARGS [1:6-1:10] 0 nil nil nil 0 nil nil (KW_ARG [1:6-1:10] (LASGN [1:6-1:10] :a (INTEGER [1:9-1:10] 1)) nil) (DVAR [1:6-1:10] nil) nil) nil)))
"def m(**k); end" => (SCOPE [1:0-1:15] [] nil (DEFN [1:0-1:15] :m (SCOPE [1:0-1:15] [:k] (ARGS [1:6-1:9] 0 nil nil nil 0 nil nil nil (DVAR [1:6-1:9] :k) nil) nil)))
"def m(**); end" => (SCOPE [1:0-1:14] [] nil (DEFN [1:0-1:14] :m (SCOPE [1:0-1:14] [:**] (ARGS [1:6-1:8] 0 nil nil nil 0 nil nil nil (DVAR [1:6-1:8] :**) nil) nil)))
"def m(**nil); end" => (SCOPE [1:0-1:17] [] nil (DEFN [1:0-1:17] :m (SCOPE [1:0-1:17] [] (ARGS [1:6-1:11] 0 nil nil nil 0 nil nil false false nil) nil)))
"def m(&b); end" => (SCOPE [1:0-1:14] [] nil (DEFN [1:0-1:14] :m (SCOPE [1:0-1:14] [:b] (ARGS [1:6-1:8] 0 nil nil nil 0 nil nil nil nil :b) nil)))
"def m(&); end" => (SCOPE [1:0-1:13] [] nil (DEFN [1:0-1:13] :m (SCOPE [1:0-1:13] [:&] (ARGS [1:6-1:7] 0 nil nil nil 0 nil nil nil nil :&) nil)))
"def m(a, *b, c); end" => (SCOPE [1:0-1:20] [] nil (DEFN [1:0-1:20] :m (SCOPE [1:0-1:20] [:a, :b, :c] (ARGS [1:6-1:14] 1 nil nil :c 1 nil :b nil nil nil) nil)))
"def m(a, (b, c)); end" => (SCOPE [1:0-1:21] [] nil (DEFN [1:0-1:21] :m (SCOPE [1:0-1:21] [:a, nil, :b, :c] (ARGS [1:6-1:15] 2 (MASGN [1:10-1:14] (LVAR [1:10-1:10] nil) (LIST [1:10-1:14] (LASGN [1:10-1:11] :b nil) (LASGN [1:13-1:14] :c nil) nil) nil) nil nil 0 nil nil nil nil nil) nil)))
"def m(a, b = 1, *c, d, e:, f: 2, **g, &h); end" => (SCOPE [1:0-1:46] [] nil (DEFN [1:0-1:46] :m (SCOPE [1:0-1:46] [:a, :b, :c, :d, :e, :f, nil, :g, :h] (ARGS [1:6-1:40] 1 nil (OPT_ARG [1:9-1:14] (LASGN [1:9-1:14] :b (INTEGER [1:13-1:14] 1)) nil) :d 1 nil :c (KW_ARG [1:23-1:31] (LASGN [1:23-1:25] :e :NODE_SPECIAL_REQUIRED_KEYWORD) (KW_ARG [1:27-1:31] (LASGN [1:27-1:31] :f (INTEGER [1:30-1:31] 2)) nil)) (DVAR [1:33-1:36] :g) :h) nil)))
"def self.m; end" => (SCOPE [1:0-1:15] [] nil (DEFS [1:0-1:15] (SELF [1:4-1:8] ) :m (SCOPE [1:0-1:15] [] (ARGS [1:10-1:10] 0 nil nil nil 0 nil nil nil nil nil) nil)))
"def m = 1" => (SCOPE [1:0-1:9] [] nil (DEFN [1:0-1:9] :m (SCOPE [1:0-1:9] [] (ARGS [1:0-1:5] 0 nil nil nil 0 nil nil nil nil nil) (INTEGER [1:8-1:9] 1))))
"def m; 1; end" => (SCOPE [1:0-1:13] [] nil (DEFN [1:0-1:13] :m (SCOPE [1:0-1:13] [] (ARGS [1:5-1:5] 0 nil nil nil 0 nil nil nil nil nil) (INTEGER [1:7-1:8] 1))))
"def m(); 1; end" => (SCOPE [1:0-1:15] [] nil (DEFN [1:0-1:15] :m (SCOPE [1:0-1:15] [] (ARGS [1:5-1:6] 0 nil nil nil 0 nil nil nil nil nil) (BLOCK [1:7-1:10] (BEGIN [1:7-1:7] nil) (INTEGER [1:9-1:10] 1)))))
"def m; rescue; end" => (SCOPE [1:0-1:18] [] nil (DEFN [1:0-1:18] :m (SCOPE [1:0-1:18] [] (ARGS [1:5-1:5] 0 nil nil nil 0 nil nil nil nil nil) (RESCUE [1:6-1:14] nil (RESBODY [1:7-1:14] nil nil (BEGIN [1:14-1:14] nil) nil) nil))))
"def m(a, ...); n(...); end" => (SCOPE [1:0-1:26] [] nil (DEFN [1:0-1:26] :m (SCOPE [1:0-1:26] [:a, :*, :**, :&, :"..."] (ARGS [1:6-1:12] 1 nil nil nil 0 nil :* nil (DVAR [1:9-1:12] :**) :&) (BLOCK [1:13-1:21] (BEGIN [1:13-1:13] nil) (FCALL [1:15-1:21] :n (BLOCK_PASS [1:16-1:21] (ARGSPUSH [1:16-1:21] (SPLAT [1:17-1:20] (LVAR [1:17-1:20] :*)) (HASH [1:17-1:20] (LIST [1:17-1:20] nil (LVAR [1:17-1:20] :**) nil))) (LVAR [1:17-1:20] :&)))))))
"def m(...) = n(...)" => (SCOPE [1:0-1:19] [] nil (DEFN [1:0-1:19] :m (SCOPE [1:0-1:19] [:*, :**, :&, :"..."] (ARGS [1:6-1:9] 0 nil nil nil 0 nil :* nil (DVAR [1:6-1:9] :**) :&) (FCALL [1:13-1:19] :n (BLOCK_PASS [1:14-1:19] (ARGSPUSH [1:14-1:19] (SPLAT [1:15-1:18] (LVAR [1:15-1:18] :*)) (HASH [1:15-1:18] (LIST [1:15-1:18] nil (LVAR [1:15-1:18] :**) nil))) (LVAR [1:15-1:18] :&))))))
"class Foo; end" => (SCOPE [1:0-1:14] [] nil (CLASS [1:0-1:14] (COLON2 [1:6-1:9] nil :Foo) nil (SCOPE [1:0-1:14] [] nil (BEGIN [1:9-1:9] nil))))
"class Foo < Bar; end" => (SCOPE [1:0-1:20] [] nil (CLASS [1:0-1:20] (COLON2 [1:6-1:9] nil :Foo) (CONST [1:12-1:15] :Bar) (SCOPE [1:0-1:20] [] nil (BEGIN [1:16-1:16] nil))))
"class Foo::Bar; end" => (SCOPE [1:0-1:19] [] nil (CLASS [1:0-1:19] (COLON2 [1:6-1:14] (CONST [1:6-1:9] :Foo) :Bar) nil (SCOPE [1:0-1:19] [] nil (BEGIN [1:14-1:14] nil))))
"class << self; end" => (SCOPE [1:0-1:18] [] nil (SCLASS [1:0-1:18] (SELF [1:9-1:13] ) (SCOPE [1:0-1:18] [] nil (BEGIN [1:14-1:14] nil))))
"module M; end" => (SCOPE [1:0-1:13] [] nil (MODULE [1:0-1:13] (COLON2 [1:7-1:8] nil :M) (SCOPE [1:0-1:13] [] nil (BEGIN [1:8-1:8] nil))))
"module M::N; end" => (SCOPE [1:0-1:16] [] nil (MODULE [1:0-1:16] (COLON2 [1:7-1:11] (CONST [1:7-1:8] :M) :N) (SCOPE [1:0-1:16] [] nil (BEGIN [1:11-1:11] nil))))
"alias a b" => (SCOPE [1:0-1:9] [] nil (ALIAS [1:0-1:9] (SYM [1:6-1:7] :a) (SYM [1:8-1:9] :b)))
"alias $a $b" => (SCOPE [1:0-1:11] [] nil (VALIAS [1:0-1:11] :$a :$b))
"undef a" => (SCOPE [1:0-1:7] [] nil (UNDEF [1:0-1:7] [#<RubyVM::AbstractSyntaxTree::Node:SYM@1:6-1:7>]))
"undef a, b" => (SCOPE [1:0-1:10] [] nil (UNDEF [1:0-1:10] [#<RubyVM::AbstractSyntaxTree::Node:SYM@1:6-1:7>, #<RubyVM::AbstractSyntaxTree::Node:SYM@1:9-1:10>]))
-- control flow
"if a then 1 else 2 end" => (SCOPE [1:0-1:22] [] nil (IF [1:0-1:22] (VCALL [1:3-1:4] :a) (INTEGER [1:10-1:11] 1) (INTEGER [1:17-1:18] 2)))
"if a; end" => (SCOPE [1:0-1:9] [] nil (IF [1:0-1:9] (VCALL [1:3-1:4] :a) (BEGIN [1:5-1:5] nil) nil))
"if a; elsif b; else; end" => (SCOPE [1:0-1:24] [] nil (IF [1:0-1:24] (VCALL [1:3-1:4] :a) (BEGIN [1:5-1:5] nil) (IF [1:6-1:20] (VCALL [1:12-1:13] :b) (BEGIN [1:14-1:14] nil) (BEGIN [1:19-1:19] nil))))
"unless a; else; end" => (SCOPE [1:0-1:19] [] nil (UNLESS [1:0-1:19] (VCALL [1:7-1:8] :a) (BEGIN [1:9-1:9] nil) (BEGIN [1:14-1:14] nil)))
"1 if a" => (SCOPE [1:0-1:6] [] nil (IF [1:0-1:6] (VCALL [1:5-1:6] :a) (INTEGER [1:0-1:1] 1) nil))
"a ? 1 : 2" => (SCOPE [1:0-1:9] [] nil (IF [1:0-1:9] (VCALL [1:0-1:1] :a) (INTEGER [1:4-1:5] 1) (INTEGER [1:8-1:9] 2)))
"while a; end" => (SCOPE [1:0-1:12] [] nil (WHILE [1:0-1:12] (VCALL [1:6-1:7] :a) (BEGIN [1:8-1:8] nil) true))
"while a do b end" => (SCOPE [1:0-1:16] [] nil (WHILE [1:0-1:16] (VCALL [1:6-1:7] :a) (VCALL [1:11-1:12] :b) true))
"until a; end" => (SCOPE [1:0-1:12] [] nil (UNTIL [1:0-1:12] (VCALL [1:6-1:7] :a) (BEGIN [1:8-1:8] nil) true))
"begin; a; end while b" => (SCOPE [1:0-1:21] [] nil (WHILE [1:0-1:21] (VCALL [1:20-1:21] :b) (BLOCK [1:5-1:8] (BEGIN [1:5-1:5] nil) (VCALL [1:7-1:8] :a)) false))
"for i in a; end" => (SCOPE [1:0-1:15] [:i] nil (FOR [1:0-1:15] (VCALL [1:9-1:10] :a) (SCOPE [1:0-1:15] [nil] (ARGS [1:4-1:5] 1 (LASGN [1:4-1:5] :i (DVAR [1:4-1:5] nil)) nil nil 0 nil nil nil nil nil) (BEGIN [1:11-1:11] nil))))
"for a, b in c; end" => (SCOPE [1:0-1:18] [:a, :b] nil (FOR [1:0-1:18] (VCALL [1:12-1:13] :c) (SCOPE [1:0-1:18] [nil] (ARGS [1:4-1:8] 0 (MASGN [1:4-1:8] (FOR_MASGN [1:4-1:8] (DVAR [1:4-1:8] nil)) (LIST [1:4-1:8] (LASGN [1:4-1:5] :a nil) (LASGN [1:7-1:8] :b nil) nil) nil) nil nil 0 nil nil nil nil nil) (BEGIN [1:14-1:14] nil))))
"case a; when 1 then 2; else 3; end" => (SCOPE [1:0-1:34] [] nil (CASE [1:0-1:34] (VCALL [1:5-1:6] :a) (WHEN [1:8-1:30] (LIST [1:13-1:14] (INTEGER [1:13-1:14] 1) nil) (INTEGER [1:20-1:21] 2) (INTEGER [1:28-1:29] 3))))
"case a; when *b then 1; end" => (SCOPE [1:0-1:27] [] nil (CASE [1:0-1:27] (VCALL [1:5-1:6] :a) (WHEN [1:8-1:23] (SPLAT [1:13-1:15] (VCALL [1:14-1:15] :b)) (INTEGER [1:21-1:22] 1) nil)))
"break" => SyntaxError
"break 1" => SyntaxError
"next" => SyntaxError
"redo" => SyntaxError
"retry" => SyntaxError
"return 1" => (SCOPE [1:0-1:8] [] nil (RETURN [1:0-1:8] (INTEGER [1:7-1:8] 1)))
"begin; a; rescue => e; b; else; c; ensure; d; end" => (SCOPE [1:0-1:49] [:e] nil (ENSURE [1:5-1:45] (RESCUE [1:5-1:33] (BLOCK [1:5-1:8] (BEGIN [1:5-1:5] nil) (VCALL [1:7-1:8] :a)) (RESBODY [1:10-1:25] nil (LASGN [1:17-1:21] :e (ERRINFO [1:17-1:21] )) (VCALL [1:23-1:24] :b) nil) (BLOCK [1:30-1:33] (BEGIN [1:30-1:30] nil) (VCALL [1:32-1:33] :c))) (BLOCK [1:41-1:44] (BEGIN [1:41-1:41] nil) (VCALL [1:43-1:44] :d))))
"begin; a; rescue A, B => e; end" => (SCOPE [1:0-1:31] [:e] nil (RESCUE [1:5-1:27] (BLOCK [1:5-1:8] (BEGIN [1:5-1:5] nil) (VCALL [1:7-1:8] :a)) (RESBODY [1:10-1:27] (LIST [1:17-1:21] (CONST [1:17-1:18] :A) (CONST [1:20-1:21] :B) nil) (LASGN [1:22-1:26] :e (ERRINFO [1:22-1:26] )) (BEGIN [1:27-1:27] nil) nil) nil))
"a rescue b" => (SCOPE [1:0-1:10] [] nil (RESCUE [1:0-1:10] (VCALL [1:0-1:1] :a) (RESBODY [1:2-1:10] nil nil (VCALL [1:9-1:10] :b) nil) nil))
"a and b" => (SCOPE [1:0-1:7] [] nil (AND [1:0-1:7] (VCALL [1:0-1:1] :a) (VCALL [1:6-1:7] :b)))
"a or b" => (SCOPE [1:0-1:6] [] nil (OR [1:0-1:6] (VCALL [1:0-1:1] :a) (VCALL [1:5-1:6] :b)))
"not a" => (SCOPE [1:0-1:5] [] nil (OPCALL [1:0-1:5] (VCALL [1:4-1:5] :a) :! nil))
"a && b" => (SCOPE [1:0-1:6] [] nil (AND [1:0-1:6] (VCALL [1:0-1:1] :a) (VCALL [1:5-1:6] :b)))
"a || b" => (SCOPE [1:0-1:6] [] nil (OR [1:0-1:6] (VCALL [1:0-1:1] :a) (VCALL [1:5-1:6] :b)))
-- pattern matching
"case a; in 1 then 2; end" => (SCOPE [1:0-1:24] [] nil (CASE3 [1:0-1:24] (VCALL [1:5-1:6] :a) (IN [1:8-1:20] (INTEGER [1:11-1:12] 1) (INTEGER [1:18-1:19] 2) nil)))
"case a; in [x, y] then 1; end" => (SCOPE [1:0-1:29] [:x, :y] nil (CASE3 [1:0-1:29] (VCALL [1:5-1:6] :a) (IN [1:8-1:25] (ARYPTN [1:12-1:16] nil (LIST [1:12-1:16] (LASGN [1:12-1:13] :x nil) (LASGN [1:15-1:16] :y nil) nil) nil nil) (INTEGER [1:23-1:24] 1) nil)))
"case a; in {k: v} then 1; end" => (SCOPE [1:0-1:29] [:v] nil (CASE3 [1:0-1:29] (VCALL [1:5-1:6] :a) (IN [1:8-1:25] (HSHPTN [1:12-1:16] nil (HASH [1:12-1:16] (LIST [1:12-1:16] (SYM [1:12-1:14] :k) (LASGN [1:15-1:16] :v nil) nil)) nil) (INTEGER [1:23-1:24] 1) nil)))
"case a; in Integer => n then n; end" => (SCOPE [1:0-1:35] [:n] nil (CASE3 [1:0-1:35] (VCALL [1:5-1:6] :a) (IN [1:8-1:31] (HASH [1:11-1:23] (LIST [1:11-1:23] (CONST [1:11-1:18] :Integer) (LASGN [1:22-1:23] :n nil) nil)) (LVAR [1:29-1:30] :n) nil)))
"case a; in 1 | 2 then 3; end" => (SCOPE [1:0-1:28] [] nil (CASE3 [1:0-1:28] (VCALL [1:5-1:6] :a) (IN [1:8-1:24] (OR [1:11-1:16] (INTEGER [1:11-1:12] 1) (INTEGER [1:15-1:16] 2)) (INTEGER [1:22-1:23] 3) nil)))
"case a; in x if x > 1 then 1; end" => (SCOPE [1:0-1:33] [:x] nil (CASE3 [1:0-1:33] (VCALL [1:5-1:6] :a) (IN [1:8-1:29] (IF [1:11-1:21] (OPCALL [1:16-1:21] (LVAR [1:16-1:17] :x) :> (LIST [1:20-1:21] (INTEGER [1:20-1:21] 1) nil)) (LASGN [1:11-1:12] :x nil) nil) (INTEGER [1:27-1:28] 1) nil)))
"case a; in []; end" => (SCOPE [1:0-1:18] [] nil (CASE3 [1:0-1:18] (VCALL [1:5-1:6] :a) (IN [1:8-1:14] (ARYPTN [1:11-1:13] nil nil nil nil) (BEGIN [1:14-1:14] nil) nil)))
"case a; in [*]; end" => (SCOPE [1:0-1:19] [] nil (CASE3 [1:0-1:19] (VCALL [1:5-1:6] :a) (IN [1:8-1:15] (ARYPTN [1:12-1:13] nil nil :NODE_SPECIAL_NO_NAME_REST nil) (BEGIN [1:15-1:15] nil) nil)))
"case a; in {}; end" => (SCOPE [1:0-1:18] [] nil (CASE3 [1:0-1:18] (VCALL [1:5-1:6] :a) (IN [1:8-1:14] (HSHPTN [1:11-1:13] nil nil nil) (BEGIN [1:14-1:14] nil) nil)))
"case a; in **nil; end" => (SCOPE [1:0-1:21] [] nil (CASE3 [1:0-1:21] (VCALL [1:5-1:6] :a) (IN [1:8-1:17] (HSHPTN [1:11-1:16] nil (HASH [1:11-1:16] nil) :NODE_SPECIAL_NO_REST_KEYWORD) (BEGIN [1:17-1:17] nil) nil)))
"case a; in ^b; end" => SyntaxError
"case a; in ^(1+1); end" => (SCOPE [1:0-1:22] [] nil (CASE3 [1:0-1:22] (VCALL [1:5-1:6] :a) (IN [1:8-1:18] (BLOCK [1:11-1:17] (OPCALL [1:13-1:16] (INTEGER [1:13-1:14] 1) :+ (LIST [1:15-1:16] (INTEGER [1:15-1:16] 1) nil))) (BEGIN [1:18-1:18] nil) nil)))
"case a; in [1, *r, 2]; end" => (SCOPE [1:0-1:26] [:r] nil (CASE3 [1:0-1:26] (VCALL [1:5-1:6] :a) (IN [1:8-1:22] (ARYPTN [1:12-1:20] nil (LIST [1:12-1:13] (INTEGER [1:12-1:13] 1) nil) (LASGN [1:15-1:17] :r nil) (LIST [1:19-1:20] (INTEGER [1:19-1:20] 2) nil)) (BEGIN [1:22-1:22] nil) nil)))
"case a; in {a:, **r}; end" => (SCOPE [1:0-1:25] [:a, :r] nil (CASE3 [1:0-1:25] (VCALL [1:5-1:6] :a) (IN [1:8-1:21] (HSHPTN [1:12-1:19] nil (HASH [1:12-1:19] (LIST [1:12-1:14] (SYM [1:12-1:14] :a) (LASGN [1:12-1:14] :a nil) nil)) (LASGN [1:12-1:19] :r nil)) (BEGIN [1:21-1:21] nil) nil)))
"case a; in Foo(x:); end" => (SCOPE [1:0-1:23] [:x] nil (CASE3 [1:0-1:23] (VCALL [1:5-1:6] :a) (IN [1:8-1:19] (HSHPTN [1:11-1:17] (CONST [1:11-1:14] :Foo) (HASH [1:15-1:17] (LIST [1:15-1:17] (SYM [1:15-1:17] :x) (LASGN [1:15-1:17] :x nil) nil)) nil) (BEGIN [1:19-1:19] nil) nil)))
"case a; in Foo[x]; end" => (SCOPE [1:0-1:22] [:x] nil (CASE3 [1:0-1:22] (VCALL [1:5-1:6] :a) (IN [1:8-1:18] (ARYPTN [1:11-1:16] (CONST [1:11-1:14] :Foo) (LIST [1:15-1:16] (LASGN [1:15-1:16] :x nil) nil) nil nil) (BEGIN [1:18-1:18] nil) nil)))
"a => b" => (SCOPE [1:0-1:6] [:b] nil (CASE3 [1:0-1:6] (VCALL [1:0-1:1] :a) (IN [1:5-1:6] (LASGN [1:5-1:6] :b nil) nil nil)))
"a in b" => (SCOPE [1:0-1:6] [:b] nil (CASE3 [1:0-1:6] (VCALL [1:0-1:1] :a) (IN [1:5-1:6] (LASGN [1:5-1:6] :b nil) (TRUE [1:5-1:6] ) (FALSE [1:5-1:6] ))))
-- hoisting and empty programs
"BEGIN { 1 }" => (SCOPE [1:0-1:11] [] nil (BLOCK [1:6-1:11] (BEGIN [1:6-1:11] (INTEGER [1:8-1:9] 1)) (BEGIN [1:6-1:11] nil)))
"BEGIN { }" => (SCOPE [1:0-1:9] [] nil (BLOCK [1:6-1:9] (BEGIN [1:6-1:9] (BEGIN [1:7-1:7] nil)) (BEGIN [1:6-1:9] nil)))
"1; BEGIN { 2 }" => (SCOPE [1:0-1:14] [] nil (BLOCK [1:9-1:14] (BEGIN [1:9-1:14] (INTEGER [1:11-1:12] 2)) (INTEGER [1:0-1:1] 1) (BEGIN [1:9-1:14] nil)))
"BEGIN { 1 }; BEGIN { 2 }" => (SCOPE [1:0-1:24] [] nil (BLOCK [1:6-1:24] (BEGIN [1:6-1:11] (INTEGER [1:8-1:9] 1)) (BEGIN [1:19-1:24] (INTEGER [1:21-1:22] 2)) (BEGIN [1:6-1:11] nil) (BEGIN [1:19-1:24] nil)))
"END { 1 }" => (SCOPE [1:0-1:9] [] nil (POSTEXE [1:0-1:9] (SCOPE [1:0-1:9] [] nil (INTEGER [1:6-1:7] 1))))
"END { }" => (SCOPE [1:0-1:7] [] nil (POSTEXE [1:0-1:7] (SCOPE [1:0-1:7] [] nil (BEGIN [1:5-1:5] nil))))
"1; END { 2 }" => (SCOPE [1:0-1:12] [] nil (BLOCK [1:0-1:12] (INTEGER [1:0-1:1] 1) (POSTEXE [1:3-1:12] (SCOPE [1:3-1:12] [] nil (INTEGER [1:9-1:10] 2)))))
"()" => (SCOPE [1:0-1:2] [] nil (BLOCK [1:0-1:2] (BEGIN [1:1-1:1] nil)))
"(a)" => (SCOPE [1:0-1:3] [] nil (BLOCK [1:0-1:3] (VCALL [1:1-1:2] :a)))
"(a; b)" => (SCOPE [1:0-1:6] [] nil (BLOCK [1:0-1:6] (BLOCK [1:1-1:5] (VCALL [1:1-1:2] :a) (VCALL [1:4-1:5] :b))))
";" => (SCOPE [1:0-1:1] [] nil (BEGIN [1:0-1:0] nil))
"a; b" => (SCOPE [1:0-1:4] [] nil (BLOCK [1:0-1:4] (VCALL [1:0-1:1] :a) (VCALL [1:3-1:4] :b)))
"a ; # comment" => (SCOPE [1:0-1:3] [] nil (VCALL [1:0-1:1] :a))
"" => (SCOPE [1:0-1:0] [] nil (BEGIN [1:0-1:0] nil))
"# only a comment\n" => (SCOPE [1:0-1:0] [] nil (BEGIN [1:0-1:0] nil))
-- multiline
"if a\n  b\nelsif c\n  d\nelse\n  e\nend" => (SCOPE [1:0-7:3] [] nil (IF [1:0-7:3] (VCALL [1:3-1:4] :a) (VCALL [2:2-2:3] :b) (IF [3:0-6:3] (VCALL [3:6-3:7] :c) (VCALL [4:2-4:3] :d) (VCALL [6:2-6:3] :e))))
"class A\n  def b\n    1\n  end\nend" => (SCOPE [1:0-5:3] [] nil (CLASS [1:0-5:3] (COLON2 [1:6-1:7] nil :A) nil (SCOPE [1:0-5:3] [] nil (BLOCK [1:7-4:5] (BEGIN [1:7-1:7] nil) (DEFN [2:2-4:5] :b (SCOPE [2:2-4:5] [] (ARGS [2:7-2:7] 0 nil nil nil 0 nil nil nil nil nil) (INTEGER [3:4-3:5] 1)))))))
"begin\n  a\nrescue X => e\n  b\nelse\n  c\nensure\n  d\nend" => (SCOPE [1:0-9:3] [:e] nil (ENSURE [2:2-8:3] (RESCUE [2:2-6:3] (VCALL [2:2-2:3] :a) (RESBODY [3:0-4:3] (LIST [3:7-3:8] (CONST [3:7-3:8] :X) nil) (LASGN [3:9-3:13] :e (ERRINFO [3:9-3:13] )) (VCALL [4:2-4:3] :b) nil) (VCALL [6:2-6:3] :c)) (VCALL [8:2-8:3] :d)))
"case a\nin {a:, **r}\n  1\nin String => s if s\n  2\nelse\n  3\nend" => (SCOPE [1:0-8:3] [:a, :r, :s] nil (CASE3 [1:0-8:3] (VCALL [1:5-1:6] :a) (IN [2:0-7:3] (HSHPTN [2:4-2:11] nil (HASH [2:4-2:11] (LIST [2:4-2:6] (SYM [2:4-2:6] :a) (LASGN [2:4-2:6] :a nil) nil)) (LASGN [2:4-2:11] :r nil)) (INTEGER [3:2-3:3] 1) (IN [4:0-7:3] (IF [4:3-4:19] (LVAR [4:18-4:19] :s) (HASH [4:3-4:14] (LIST [4:3-4:14] (CONST [4:3-4:9] :String) (LASGN [4:13-4:14] :s nil) nil)) nil) (INTEGER [5:2-5:3] 2) (INTEGER [7:2-7:3] 3)))))
"x = <<~E\n  a\nE\n" => (SCOPE [1:0-1:8] [:x] nil (LASGN [1:0-1:8] :x (STR [1:4-1:8] "a\n")))

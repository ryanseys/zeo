# The cycle this program builds on purpose, and the census the
# `ZEO_RT_GCCHECK=1` leg gates against. A cycle alive at exit is not a
# defect: it is a ring the program never broke. What the leg gates is a
# CHANGE to the line below.
# 
# Three self-referential documents: a sequence that aliases itself, a mapping that aliases itself, and the `a` / `b` pair where `c: *a` closes the ring.
#@ gccheck: cycle leak: 4 objects (Hash x3, Array x1)
# Every hostile shape the YAML loader has to survive, as ANSWERS rather
# than as a promise. The point of the file is that none of these ends the
# process: each row is either a value or a Ruby exception, and the golden
# pins which.
#
# Written when the loader was reworked to read the anchor id and the tag
# that yaml-rust2's events always carried, and it found real defects rather
# than confirming a design -- see `yaml_parser_stress.rb` for the generated
# half.
#
# ONE GROUP IS NOT HERE: the documents yaml-rust2 and libyaml disagree
# about the legality of, which is a decided divergence recorded in
# `tests/yaml_backend_accepts_a_different_dialect.rb`.

require "yaml"
require "date"

def show(name)
  r = yield
  puts "#{name}\t#{r.inspect}"
rescue Exception => e
  puts "#{name}\t#{e.class}"
end

# --- Truncation, at every position a document has ------------------------
[
  "", " ", "---", "--- ", "-", "- ", "a:", "a: ", "{", "}", "[", "]",
  "{a", "{a:", "[1", "'", "\"", "\"a", "'a", "&", "&a", "*", "*a", "!",
  "!!", "!!str", "? ", "|", ">", "a: &", "a: *", "%YAML", "%YAML 1.1",
  "---\n..."
].each do |frag|
  show("truncated #{frag.inspect}") { YAML.unsafe_load(frag) }
end

# --- Anchors and aliases -------------------------------------------------
show("simple alias") { YAML.unsafe_load("a: &x 1\nb: *x") }
show("container alias") { YAML.unsafe_load("a: &x [1]\nb: *x") }
show("alias shares identity") do
  v = YAML.unsafe_load("a: &x [1]\nb: *x")
  v["a"].equal?(v["b"])
end
show("self-referential seq") do
  v = YAML.unsafe_load("--- &1\n- *1\n")
  [v.size, v.first.equal?(v)]
end
show("self-referential map") do
  v = YAML.unsafe_load("--- &1\na: *1\n")
  v["a"].equal?(v)
end
show("mutual") do
  v = YAML.unsafe_load("a: &a\n  b: &b\n    c: *a\n")
  v["a"]["b"]["c"].equal?(v["a"])
end
show("anchor reused") { YAML.unsafe_load("a: &x 1\nb: &x 2\nc: *x") }
show("undefined anchor") { YAML.unsafe_load("b: *nope") }
show("anchor on nothing") { YAML.unsafe_load("a: &x\nb: *x") }
show("alias without option") { YAML.safe_load("a: &x 1\nb: *x") }
show("many aliases") { YAML.unsafe_load("a: &x [1]\n" + (0...50).map { |i| "k#{i}: *x" }.join("\n")).size }

# --- Merge keys ----------------------------------------------------------
show("merge") { YAML.unsafe_load("b: &b {x: 1}\nc:\n  <<: *b\n  y: 2") }
show("merge overrides") { YAML.unsafe_load("b: &b {x: 1}\nc:\n  <<: *b\n  x: 9") }
show("merge list") { YAML.unsafe_load("a: &a {x: 1}\nb: &b {x: 2, y: 3}\nc:\n  <<: [*a, *b]") }
show("merge inline") { YAML.unsafe_load("c:\n  <<: {x: 1}") }
show("merge quoted key") { YAML.unsafe_load("c:\n  !!str '<<': 1") }
show("merge of a scalar") { YAML.unsafe_load("c:\n  <<: 1") }
show("merge of a seq of scalars") { YAML.unsafe_load("c:\n  <<: [1, 2]") }
show("merge twice") { YAML.unsafe_load("a: &a {x: 1}\nb: &b {y: 2}\nc:\n  <<: *a\n  <<: *b") }
show("merge self") { YAML.unsafe_load("c: &c\n  <<: *c\n  x: 1") }

# --- Tags ----------------------------------------------------------------
show("tag str on a number") { YAML.unsafe_load("v: !!str 017")["v"] }
show("tag int on text") { YAML.unsafe_load("v: !!int '42'")["v"] }
show("tag int on garbage") { YAML.unsafe_load("v: !!int 'zz'")["v"] }
show("tag float on int") { YAML.unsafe_load("v: !!float 3")["v"] }
show("tag float on garbage") { YAML.unsafe_load("v: !!float 'zz'")["v"] }
show("tag bool") { YAML.unsafe_load("v: !!bool yes")["v"] }
show("tag binary") { YAML.unsafe_load("v: !!binary aGk=")["v"] }
show("tag binary garbage") { YAML.unsafe_load("v: !!binary '@@@'")["v"] }
show("tag binary empty") { YAML.unsafe_load("v: !!binary ''")["v"] }
show("tag omap") { YAML.unsafe_load("v: !!omap\n- a: 1\n- b: 2")["v"] }
show("tag set") { YAML.unsafe_load("v: !!set\n  ? a\n  ? b")["v"] }
show("tag unknown") { YAML.unsafe_load("v: !whatever 42")["v"] }
show("tag ruby symbol") { YAML.unsafe_load("v: !ruby/symbol foo")["v"] }
show("tag on a seq") { YAML.unsafe_load("v: !!seq [1]")["v"] }

# --- Documents -----------------------------------------------------------
show("two documents") { YAML.unsafe_load("--- 1\n--- 2\n") }
show("load_stream") { YAML.load_stream("--- 1\n--- 2\n") }
show("empty document") { YAML.unsafe_load("---\n") }
show("only a comment") { YAML.unsafe_load("# nothing\n") }
show("directive") { YAML.unsafe_load("%YAML 1.1\n--- 1\n") }
show("explicit end") { YAML.unsafe_load("--- 1\n...\n") }

# --- Structure -----------------------------------------------------------
show("duplicate keys") { YAML.unsafe_load("a: 1\na: 2") }
show("complex key") { YAML.unsafe_load("? [1, 2]\n: 3") }
show("nested flow") { YAML.unsafe_load("[{a: [1, {b: 2}]}]") }
show("empty flow") { YAML.unsafe_load("[[], {}, [{}]]") }
show("mixed indent") { YAML.unsafe_load("a:\n  b: 1\n   c: 2") }
show("deep but legal") { YAML.unsafe_load("[" * 50 + "]" * 50).flatten.size }

# --- Bytes ---------------------------------------------------------------
show("long scalar") { YAML.unsafe_load("v: #{'x' * 200_000}")["v"].size }

# --- Round trips ---------------------------------------------------------
[
  { "a" => 1 },
  { "a" => [1, 2, { "b" => nil }] },
  [1.5, -0.0, true, false, nil, "x"],
  { "s" => "l1\nl2\n" },
  { "s" => "l1\nl2" },
  { nil => 1 },
  { :sym => :other },
  "héllo",
  ["", " x", "x ", "017", "true", "a: b"],
  1..3
].each_with_index do |v, i|
  show("round trip #{i}") { YAML.unsafe_load(YAML.dump(v)) == v }
end
__END__
truncated ""	false
truncated " "	false
truncated "---"	nil
truncated "--- "	nil
truncated "-"	[nil]
truncated "- "	[nil]
truncated "a:"	{"a" => nil}
truncated "a: "	{"a" => nil}
truncated "{"	Psych::SyntaxError
truncated "}"	Psych::SyntaxError
truncated "["	Psych::SyntaxError
truncated "]"	Psych::SyntaxError
truncated "{a"	Psych::SyntaxError
truncated "{a:"	Psych::SyntaxError
truncated "[1"	Psych::SyntaxError
truncated "'"	Psych::SyntaxError
truncated "\""	Psych::SyntaxError
truncated "\"a"	Psych::SyntaxError
truncated "'a"	Psych::SyntaxError
truncated "&"	Psych::SyntaxError
truncated "&a"	nil
truncated "*"	Psych::SyntaxError
truncated "*a"	Psych::AnchorNotDefined
truncated "!"	nil
truncated "!!"	Psych::SyntaxError
truncated "!!str"	""
truncated "? "	{nil => nil}
truncated "|"	""
truncated ">"	""
truncated "a: &"	Psych::SyntaxError
truncated "a: *"	Psych::SyntaxError
truncated "%YAML"	Psych::SyntaxError
truncated "%YAML 1.1"	Psych::SyntaxError
truncated "---\n..."	nil
simple alias	{"a" => 1, "b" => 1}
container alias	{"a" => [1], "b" => [1]}
alias shares identity	true
self-referential seq	[1, true]
self-referential map	true
mutual	true
anchor reused	{"a" => 1, "b" => 2, "c" => 2}
undefined anchor	Psych::AnchorNotDefined
anchor on nothing	{"a" => nil, "b" => nil}
alias without option	Psych::AliasesNotEnabled
many aliases	51
merge	{"b" => {"x" => 1}, "c" => {"x" => 1, "y" => 2}}
merge overrides	{"b" => {"x" => 1}, "c" => {"x" => 9}}
merge list	{"a" => {"x" => 1}, "b" => {"x" => 2, "y" => 3}, "c" => {"x" => 1, "y" => 3}}
merge inline	{"c" => {"x" => 1}}
merge quoted key	{"c" => {"<<" => 1}}
merge of a scalar	{"c" => {"<<" => 1}}
merge of a seq of scalars	{"c" => {"<<" => [1, 2]}}
merge twice	{"a" => {"x" => 1}, "b" => {"y" => 2}, "c" => {"x" => 1, "y" => 2}}
merge self	{"c" => {"x" => 1}}
tag str on a number	"017"
tag int on text	42
tag int on garbage	"zz"
tag float on int	3.0
tag float on garbage	ArgumentError
tag bool	true
tag binary	"hi"
tag binary garbage	""
tag binary empty	""
tag omap	{"a" => 1, "b" => 2}
tag set	{"a" => nil, "b" => nil}
tag unknown	42
tag ruby symbol	:foo
tag on a seq	[1]
two documents	1
load_stream	[1, 2]
empty document	nil
only a comment	false
directive	1
explicit end	1
duplicate keys	{"a" => 2}
complex key	{[1, 2] => 3}
nested flow	[{"a" => [1, {"b" => 2}]}]
empty flow	[[], {}, [{}]]
mixed indent	Psych::SyntaxError
deep but legal	0
long scalar	200000
round trip 0	true
round trip 1	true
round trip 2	true
round trip 3	true
round trip 4	true
round trip 5	true
round trip 6	true
round trip 7	true
round trip 8	true
round trip 9	true

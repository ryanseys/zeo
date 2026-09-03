# `Psych.parse` and the `Psych::Nodes::*` tree it answers.
#
# The tree is the DOCUMENT rather than the data: a gem walks it to read a tag,
# rewrite a scalar or count anchors, and `to_ruby` turns any node -- parsed or
# hand-built -- into the value it describes.

require "psych"

def show(label, v)
  puts "#{label}\t#{v.inspect}"
end

# --- the shape of a parsed document --------------------------------------
doc = Psych.parse("--- \na: 1\nb: [2, 3]\n")
show "document class", doc.class
show "document children", doc.children.size
show "implicit", doc.implicit
show "version", doc.version
show "tag directives", doc.tag_directives

root = doc.root
show "root class", root.class
show "root style block", root.style == Psych::Nodes::Mapping::BLOCK
show "root children", root.children.size

key = root.children[0]
show "key class", key.class
show "key value", key.value
show "key plain", key.plain
show "key quoted", key.quoted
show "key style plain", key.style == Psych::Nodes::Scalar::PLAIN

seq = root.children[3]
show "seq class", seq.class
show "seq style flow", seq.style == Psych::Nodes::Sequence::FLOW
show "seq children", seq.children.map(&:value)

# --- quoting and block scalars keep their style ---------------------------
styles = Psych.parse("- plain\n- 'single'\n- \"double\"\n- |\n  literal\n- >\n  folded\n")
show "styles", styles.root.children.map(&:style)
show "quoted flags", styles.root.children.map(&:quoted)

# --- anchors come back as NAMES, not numbers ------------------------------
anchored = Psych.parse("a: &base {x: 1}\nb: *base\n")
pairs = anchored.root.children
show "anchor name", pairs[1].anchor
show "alias class", pairs[3].class
show "alias anchor", pairs[3].anchor

# --- tags survive ---------------------------------------------------------
tagged = Psych.parse("--- !!str 42\n")
show "scalar tag", tagged.root.tag
show "scalar value", tagged.root.value

taggedmap = Psych.parse("--- !ruby/object:Widget\nname: bolt\n")
show "mapping tag", taggedmap.root.tag

# --- an explicit document says so ----------------------------------------
show "explicit implicit?", Psych.parse("--- \na: 1\n").implicit
show "bare implicit?", Psych.parse("a: 1\n").implicit

# --- directives -----------------------------------------------------------
withdirs = Psych.parse("%YAML 1.1\n%TAG !e! tag:example.com,2000:\n---\na: 1\n")
show "version", withdirs.version
show "tag directives", withdirs.tag_directives

# --- the whole stream -----------------------------------------------------
stream = Psych.parse_stream("--- 1\n--- 2\n")
show "stream class", stream.class
show "stream documents", stream.children.size
show "stream roots", stream.children.map { |d| d.root.value }

collected = []
Psych.parse_stream("--- 1\n--- 2\n") { |d| collected << d.root.value }
show "yielded", collected

# --- an empty stream ------------------------------------------------------
show "empty parse", Psych.parse("")

# --- each walks depth first, self first -----------------------------------
show "each classes", Psych.parse("a: [1]\n").each.map { |n| n.class.to_s.split("::").last }

# --- to_ruby on a parsed tree --------------------------------------------
show "document to_ruby", Psych.parse("a: 1\nb: [2, 3]\n").to_ruby
show "root to_ruby", Psych.parse("a: 1\n").root.to_ruby
show "scalar to_ruby", Psych.parse("--- 42\n").root.to_ruby
show "stream to_ruby", Psych.parse_stream("--- 7\n").to_ruby

# --- to_ruby on a tree built by hand -------------------------------------
built = Psych::Nodes::Mapping.new
built.children << Psych::Nodes::Scalar.new("k")
built.children << Psych::Nodes::Scalar.new("9")
show "hand-built to_ruby", built.to_ruby

wrapped = Psych::Nodes::Document.new
wrapped.children << built
show "hand-built document", wrapped.to_ruby

# --- an alias in a hand-built tree resolves ------------------------------
aliased = Psych::Nodes::Mapping.new
aliased.children << Psych::Nodes::Scalar.new("a")
aliased.children << Psych::Nodes::Sequence.new("keep")
aliased.children << Psych::Nodes::Scalar.new("b")
aliased.children << Psych::Nodes::Alias.new("keep")
built_alias = aliased.to_ruby
show "alias resolved", built_alias["a"].equal?(built_alias["b"])

# `Nodes::Node#yaml` is deliberately absent from these rows: zeo refuses it
# (see tests/gaps/a_psych_node_tree_emits_yaml.rb) and ruby raises a
# different error again for a bare Document, so the row would compare two
# unrelated refusals rather than one behaviour.
__END__
document class	Psych::Nodes::Document
document children	1
implicit	false
version	[]
tag directives	[]
root class	Psych::Nodes::Mapping
root style block	true
root children	4
key class	Psych::Nodes::Scalar
key value	"a"
key plain	true
key quoted	false
key style plain	true
seq class	Psych::Nodes::Sequence
seq style flow	true
seq children	["2", "3"]
styles	[1, 2, 3, 4, 5]
quoted flags	[false, true, true, true, true]
anchor name	"base"
alias class	Psych::Nodes::Alias
alias anchor	"base"
scalar tag	"tag:yaml.org,2002:str"
scalar value	"42"
mapping tag	"!ruby/object:Widget"
explicit implicit?	false
bare implicit?	true
version	[1, 1]
tag directives	[["!e!", "tag:example.com,2000:"]]
stream class	Psych::Nodes::Stream
stream documents	2
stream roots	["1", "2"]
yielded	["1", "2"]
empty parse	false
each classes	["Scalar", "Scalar", "Sequence", "Mapping", "Document"]
document to_ruby	{"a" => 1, "b" => [2, 3]}
root to_ruby	{"a" => 1}
scalar to_ruby	42
stream to_ruby	[7]
hand-built to_ruby	{"k" => 9}
hand-built document	{"k" => 9}
alias resolved	true

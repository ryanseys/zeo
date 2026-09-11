# Every node `Psych.parse_stream` builds reports where it starts and ends,
# as a 0-based line and a column counted in characters. A scalar ends after
# its last character or closing quote, a block scalar after its lines, a flow
# container after its bracket, and a block container where the next token
# starts. A node with an anchor or a tag starts at that property.
require "psych"

def walk(node, depth = 0)
  label = node.class.name.split("::").last
  extra = node.respond_to?(:value) ? " #{node.value.inspect}" : ""
  extra += " *#{node.anchor}" if node.is_a?(Psych::Nodes::Alias)
  puts "#{"  " * depth}#{label}#{extra} #{node.start_line}:#{node.start_column}->#{node.end_line}:#{node.end_column}"
  node.children.each { |c| walk(c, depth + 1) } if node.children
end

[
  "a: hello\nb: 2\n",
  "- x\n- [1, 2]\n",
  "k: {a: b}\n",
  "a: |\n  t\n",
  "a: >\n  t\n  u\n\nb: 1\n",
  "'q': \"d\\n\"\n",
  "--- !foo &a x\n...\n",
  "a: &x 1\nb: *x\n",
  "hello",
  "  a: 1\n  b:\n    - 2\n",
  "---\na: 1\n--- b\n",
  "a:\nb: ~\n",
  "é: ü\n",
  "- plain  text   \n- 'multi\n  line'\n",
  "x: [a, {b: c}, [d]]\n",
  "",
  "# only a comment\n",
  "-\n- x\n",
  "a: !!str\nb: 1\n",
  "# c\na: 1 # tail\nb: # why\n  c: 2\n",
  "&m\na: 1\n",
  "k: [a,\n  b]\n",
  "a: |+\n  t\n\n",
  "a: \"x\\\"y\"\n",
  "- - a\n  - b\n- c\n",
  "k:\n- a\n- b\nz: 1\n",
  "{a: }\n",
  "%YAML 1.1\n---\na: 1\n",
  "a: 'it''s'\n",
  "a: multi\n  line plain\nb: 2\n",
].each do |src|
  puts "== #{src.inspect}"
  walk(Psych.parse_stream(src))
end

doc = Psych.parse("a: hello\n")
key = doc.root.children[0]
puts "parse: #{key.start_line}:#{key.start_column}->#{key.end_line}:#{key.end_column}"
__END__
== "a: hello\nb: 2\n"
Stream 0:0->2:0
  Document 0:0->2:0
    Mapping 0:0->2:0
      Scalar "a" 0:0->0:1
      Scalar "hello" 0:3->0:8
      Scalar "b" 1:0->1:1
      Scalar "2" 1:3->1:4
== "- x\n- [1, 2]\n"
Stream 0:0->2:0
  Document 0:0->2:0
    Sequence 0:0->2:0
      Scalar "x" 0:2->0:3
      Sequence 1:2->1:8
        Scalar "1" 1:3->1:4
        Scalar "2" 1:6->1:7
== "k: {a: b}\n"
Stream 0:0->1:0
  Document 0:0->1:0
    Mapping 0:0->1:0
      Scalar "k" 0:0->0:1
      Mapping 0:3->0:9
        Scalar "a" 0:4->0:5
        Scalar "b" 0:7->0:8
== "a: |\n  t\n"
Stream 0:0->2:0
  Document 0:0->2:0
    Mapping 0:0->2:0
      Scalar "a" 0:0->0:1
      Scalar "t\n" 0:3->2:0
== "a: >\n  t\n  u\n\nb: 1\n"
Stream 0:0->5:0
  Document 0:0->5:0
    Mapping 0:0->5:0
      Scalar "a" 0:0->0:1
      Scalar "t u\n" 0:3->4:0
      Scalar "b" 4:0->4:1
      Scalar "1" 4:3->4:4
== "'q': \"d\\n\"\n"
Stream 0:0->1:0
  Document 0:0->1:0
    Mapping 0:0->1:0
      Scalar "q" 0:0->0:3
      Scalar "d\n" 0:5->0:10
== "--- !foo &a x\n...\n"
Stream 0:0->2:0
  Document 0:0->1:3
    Scalar "x" 0:4->0:13
== "a: &x 1\nb: *x\n"
Stream 0:0->2:0
  Document 0:0->2:0
    Mapping 0:0->2:0
      Scalar "a" 0:0->0:1
      Scalar "1" 0:3->0:7
      Scalar "b" 1:0->1:1
      Alias *x 1:3->1:5
== "hello"
Stream 0:0->1:0
  Document 0:0->1:0
    Scalar "hello" 0:0->0:5
== "  a: 1\n  b:\n    - 2\n"
Stream 0:0->3:0
  Document 0:2->3:0
    Mapping 0:2->3:0
      Scalar "a" 0:2->0:3
      Scalar "1" 0:5->0:6
      Scalar "b" 1:2->1:3
      Sequence 2:4->3:0
        Scalar "2" 2:6->2:7
== "---\na: 1\n--- b\n"
Stream 0:0->3:0
  Document 0:0->2:0
    Mapping 1:0->2:0
      Scalar "a" 1:0->1:1
      Scalar "1" 1:3->1:4
  Document 2:0->3:0
    Scalar "b" 2:4->2:5
== "a:\nb: ~\n"
Stream 0:0->2:0
  Document 0:0->2:0
    Mapping 0:0->2:0
      Scalar "a" 0:0->0:1
      Scalar "" 0:2->0:2
      Scalar "b" 1:0->1:1
      Scalar "~" 1:3->1:4
== "é: ü\n"
Stream 0:0->1:0
  Document 0:0->1:0
    Mapping 0:0->1:0
      Scalar "é" 0:0->0:1
      Scalar "ü" 0:3->0:4
== "- plain  text   \n- 'multi\n  line'\n"
Stream 0:0->3:0
  Document 0:0->3:0
    Sequence 0:0->3:0
      Scalar "plain  text" 0:2->0:13
      Scalar "multi line" 1:2->2:7
== "x: [a, {b: c}, [d]]\n"
Stream 0:0->1:0
  Document 0:0->1:0
    Mapping 0:0->1:0
      Scalar "x" 0:0->0:1
      Sequence 0:3->0:19
        Scalar "a" 0:4->0:5
        Mapping 0:7->0:13
          Scalar "b" 0:8->0:9
          Scalar "c" 0:11->0:12
        Sequence 0:15->0:18
          Scalar "d" 0:16->0:17
== ""
Stream 0:0->0:0
== "# only a comment\n"
Stream 0:0->1:0
== "-\n- x\n"
Stream 0:0->2:0
  Document 0:0->2:0
    Sequence 0:0->2:0
      Scalar "" 0:1->0:1
      Scalar "x" 1:2->1:3
== "a: !!str\nb: 1\n"
Stream 0:0->2:0
  Document 0:0->2:0
    Mapping 0:0->2:0
      Scalar "a" 0:0->0:1
      Scalar "" 0:3->0:8
      Scalar "b" 1:0->1:1
      Scalar "1" 1:3->1:4
== "# c\na: 1 # tail\nb: # why\n  c: 2\n"
Stream 0:0->4:0
  Document 1:0->4:0
    Mapping 1:0->4:0
      Scalar "a" 1:0->1:1
      Scalar "1" 1:3->1:4
      Scalar "b" 2:0->2:1
      Mapping 3:2->4:0
        Scalar "c" 3:2->3:3
        Scalar "2" 3:5->3:6
== "&m\na: 1\n"
Stream 0:0->2:0
  Document 0:0->2:0
    Mapping 0:0->2:0
      Scalar "a" 1:0->1:1
      Scalar "1" 1:3->1:4
== "k: [a,\n  b]\n"
Stream 0:0->2:0
  Document 0:0->2:0
    Mapping 0:0->2:0
      Scalar "k" 0:0->0:1
      Sequence 0:3->1:4
        Scalar "a" 0:4->0:5
        Scalar "b" 1:2->1:3
== "a: |+\n  t\n\n"
Stream 0:0->3:0
  Document 0:0->3:0
    Mapping 0:0->3:0
      Scalar "a" 0:0->0:1
      Scalar "t\n\n" 0:3->3:0
== "a: \"x\\\"y\"\n"
Stream 0:0->1:0
  Document 0:0->1:0
    Mapping 0:0->1:0
      Scalar "a" 0:0->0:1
      Scalar "x\"y" 0:3->0:9
== "- - a\n  - b\n- c\n"
Stream 0:0->3:0
  Document 0:0->3:0
    Sequence 0:0->3:0
      Sequence 0:2->2:0
        Scalar "a" 0:4->0:5
        Scalar "b" 1:4->1:5
      Scalar "c" 2:2->2:3
== "k:\n- a\n- b\nz: 1\n"
Stream 0:0->4:0
  Document 0:0->4:0
    Mapping 0:0->4:0
      Scalar "k" 0:0->0:1
      Sequence 1:0->3:0
        Scalar "a" 1:2->1:3
        Scalar "b" 2:2->2:3
      Scalar "z" 3:0->3:1
      Scalar "1" 3:3->3:4
== "{a: }\n"
Stream 0:0->1:0
  Document 0:0->1:0
    Mapping 0:0->0:5
      Scalar "a" 0:1->0:2
      Scalar "" 0:4->0:4
== "%YAML 1.1\n---\na: 1\n"
Stream 0:0->3:0
  Document 0:0->3:0
    Mapping 2:0->3:0
      Scalar "a" 2:0->2:1
      Scalar "1" 2:3->2:4
== "a: 'it''s'\n"
Stream 0:0->1:0
  Document 0:0->1:0
    Mapping 0:0->1:0
      Scalar "a" 0:0->0:1
      Scalar "it's" 0:3->0:10
== "a: multi\n  line plain\nb: 2\n"
Stream 0:0->3:0
  Document 0:0->3:0
    Mapping 0:0->3:0
      Scalar "a" 0:0->0:1
      Scalar "multi line plain" 0:3->1:12
      Scalar "b" 2:0->2:1
      Scalar "2" 2:3->2:4
parse: 0:0->0:1

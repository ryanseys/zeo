# The emitter leaves a document's `---` off only where libyaml does: an
# implicit document, first in its stream, with no directives. A later
# document, an explicit one, or one with `%YAML`/`%TAG` directives gets the
# marker, the directives lead it, and a `%TAG` handle shortens the tags it
# prefixes. A document emitted outside a stream is refused.
require "psych"

def show(label)
  out = begin
    yield
  rescue => e
    "#{e.class}: #{e.message}"
  end
  puts "== #{label}"
  puts out.inspect
end

show("implicit in built stream") { s = Psych::Nodes::Stream.new; s.children << Psych.parse("a: 1\n"); s.yaml }
show("explicit parsed") { s = Psych::Nodes::Stream.new; s.children << Psych.parse("---\na: 1\n"); s.yaml }
show("two implicit") { s = Psych::Nodes::Stream.new; s.children << Psych.parse("a: 1\n") << Psych.parse("b: 2\n"); s.yaml }
show("scalar root implicit") { s = Psych::Nodes::Stream.new; s.children << Psych.parse("hello\n"); s.yaml }
show("sequence root implicit") { s = Psych::Nodes::Stream.new; s.children << Psych.parse("- 1\n- 2\n"); s.yaml }
show("%YAML directive") { Psych.parse_stream("%YAML 1.1\n---\na: 1\n").yaml }
show("%TAG directive") { Psych.parse_stream("%TAG ! tag:example.com,2000:\n--- !foo\na: 1\n").yaml }
show("explicit end") { Psych.parse_stream("a: 1\n...\n").yaml }
show("bare document") { Psych.parse("a: 1\n").yaml }
show("to_yaml") { {a: 1}.to_yaml }
show("dump scalar") { Psych.dump(1) }
__END__
== implicit in built stream
"a: 1\n"
== explicit parsed
"---\na: 1\n"
== two implicit
"a: 1\n---\nb: 2\n"
== scalar root implicit
"hello\n"
== sequence root implicit
"- 1\n- 2\n"
== %YAML directive
"%YAML 1.1\n---\na: 1\n"
== %TAG directive
"%TAG ! tag:example.com,2000:\n--- !foo\na: 1\n"
== explicit end
"a: 1\n...\n"
== bare document
"RuntimeError: expected STREAM-START"
== to_yaml
"---\n:a: 1\n"
== dump scalar
"--- 1\n"

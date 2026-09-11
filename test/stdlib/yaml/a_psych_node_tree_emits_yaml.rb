# A `Psych::Nodes::Stream` built by hand, holding a document that
# `Psych.parse` produced, emits that document's YAML: the document is
# implicit and first, so it carries no `---` start marker.
require "psych"

stream = Psych::Nodes::Stream.new
stream.children << Psych.parse("a: 1\n")
puts stream.yaml
__END__
a: 1

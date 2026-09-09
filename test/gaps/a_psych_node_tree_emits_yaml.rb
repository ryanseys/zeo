# A `Psych::Nodes::Stream` built by hand, holding a document that
# `Psych.parse` produced, emits that document's YAML. zeo's emitter walks the
# node tree itself rather than driving libyaml, and does not yet handle a
# stream whose children were assembled rather than parsed.
require "psych"

stream = Psych::Nodes::Stream.new
stream.children << Psych.parse("a: 1\n")
puts stream.yaml
__END__
a: 1

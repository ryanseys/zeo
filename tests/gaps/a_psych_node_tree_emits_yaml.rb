require "psych"

stream = Psych::Nodes::Stream.new
stream.children << Psych.parse("a: 1\n")
puts stream.yaml

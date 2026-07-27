# YAML.dump correctly emits symbol keys/values using the `:name` tag syntax,
# but YAML.load doesn't parse that syntax back into Symbol -- it comes back
# as the literal string ":x" instead of the Symbol :x.
require "yaml"
dumped = YAML.dump({x: "hi", y: 2})
puts dumped
p YAML.load(dumped)

# `YAML.dump` emits a Symbol as the plain scalar `:name`, and `YAML.load` reads
# that spelling back as a Symbol -- so a Hash with symbol keys survives the
# round trip. The QUOTING is what carries the distinction: a quoted `":x"` is
# still a String, which is why the load side has to read the parser's scalar
# style rather than just the text.
require "yaml"

dumped = YAML.dump({x: "hi", y: 2})
puts dumped
p YAML.load(dumped)

[":x", ":X", ":a-b", ":1", ":@iv", ":+", ":x y", "::a"].each { |s| p [s, YAML.load(s)] }

p YAML.dump(:"with space")
p YAML.load(YAML.dump(:"with space"))

# Quoted, and block, scalars stay Strings.
p YAML.load("- :a\n- \":b\"\n- ':c'\n")
p YAML.load(":a: 1\n\":b\": 2\n")
p YAML.load("--- |\n  :not a symbol\n")
p YAML.load("--- >\n  :also not\n")

# Nesting, in both key and value position.
p YAML.load(YAML.dump({a: {b: [:c, "d"]}}))
p YAML.load("? - :k\n: :v\n")
p YAML.load(YAML.dump([{one: 1}, {two: :owt}]))

# The rest of the plain-scalar scanner is unchanged.
p YAML.load("- 1\n- ~\n- null\n- true\n- False\n- 0x1f\n- 3.5\n- .inf\n- -.inf\n- ''\n")
["1_000", "_1", "1_", "1__0", "-1_2", "1_000.5", "a_1"].each { |s| p [s, YAML.load(s)] }

p YAML.load_stream("---\n:a: 1\n---\n- :b\n")
p YAML.load("")
p YAML.load("--- {}\n")
p YAML.load("--- []\n")

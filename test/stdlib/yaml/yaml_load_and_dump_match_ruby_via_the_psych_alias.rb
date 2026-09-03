# `require "yaml"` exposes both `YAML` and `Psych` (Ruby's `yaml.rb` is
# `YAML = Psych`); dump uses Psych's block style (sequences under a key
# stay at the key's indent).

require "yaml"
print YAML.dump({"a" => 1, "b" => [2, "x"]})
puts YAML.load("list:\n- a\n- b").inspect
puts Psych.load("x: 1").inspect
__END__
---
a: 1
b:
- 2
- x
{"list" => ["a", "b"]}
{"x" => 1}

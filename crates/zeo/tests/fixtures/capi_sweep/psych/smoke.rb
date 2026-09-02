require "psych"

h = {"a" => [1, 2.5, nil, true], "b" => {"c" => "d"}, "s" => "multi\nline"}
y = Psych.dump(h)
puts y
p Psych.load(y)
p Psych.safe_load("- 1\n- two\n- 3.0\n")
p Psych.load("x: &a [1]\ny: *a\n", aliases: true)
begin
  Psych.load("a: [")
rescue Psych::SyntaxError => e
  puts e.class
end
puts Psych::LIBYAML_VERSION.class, Psych.libyaml_version.size

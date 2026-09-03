# The json extension links into the binary: generation and parsing both run
# native code the link line has to carry.
require "json"
doc = { "name" => "zeo", "tags" => %w[ruby aot], "n" => 42, "ok" => true, "nil" => nil }
text = JSON.generate(doc)
puts text
p JSON.parse(text) == doc
puts JSON.pretty_generate("a" => [1, 2])
p JSON.parse('{"deep":{"list":[1,2,3]}}', symbolize_names: true)
__END__
{"name":"zeo","tags":["ruby","aot"],"n":42,"ok":true,"nil":null}
true
{
  "a": [
    1,
    2
  ]
}
{deep: {list: [1, 2, 3]}}

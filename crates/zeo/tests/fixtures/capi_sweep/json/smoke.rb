require "json"

s = JSON.generate({"a" => [1, 2.5, nil, true], "b" => {"c" => "dé"}})
puts s
p JSON.parse(s)
puts JSON.pretty_generate([1, {"x" => 2}])
p JSON.parse('{"a": 1, "b": [true, null]}', symbolize_names: true)
p JSON.parse("1e3"), JSON.parse("\"\\u00e9\"")
begin
  JSON.parse("{")
rescue JSON::ParserError => e
  puts e.class
end
puts({"k" => 1}.to_json, [1, "two"].to_json, "é\n".to_json)
puts JSON::Parser.name, JSON::State.name

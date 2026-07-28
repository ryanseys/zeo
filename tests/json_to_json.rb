# `require "json"` gives every object a `#to_json`, not just the explicit
# `JSON.generate`/`JSON.dump` entry points.
require "json"

p({a: 1}.to_json)
p({a: {b: [1, nil]}}.to_json)
p [1, "two", nil, true, false].to_json
p 1.to_json, 1.5.to_json, "hi".to_json, :sym.to_json
p nil.to_json, true.to_json, false.to_json

# Anything else encodes as its `to_s`, as a JSON STRING.
p (1..3).to_json
p Struct.new(:x).new(1).to_json
p Object.new.to_json.class

# ...and the state argument a container threads through is accepted.
p({a: 1}.to_json(nil))
p [1].to_json(nil)

# Round-tripping through the parser gives the value back.
p JSON.parse({"a" => [1, {"b" => nil}]}.to_json)
p JSON.parse("[1,2]").to_json

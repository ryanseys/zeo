# `require "json"` adds `#to_json` to the core classes, so a Hash serializes
# through the method as well as through the explicit `JSON.generate` and
# `JSON.dump` entry points.
require "json"
p({a: 1}.to_json)

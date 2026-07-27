# require "json" adds Kernel#to_json to core objects (Hash, Array, etc) via
# JSON.generate, but zeo doesn't wire up Hash#to_json -- only the explicit
# JSON.generate/JSON.dump entry points work.
require "json"
p({a: 1}.to_json)

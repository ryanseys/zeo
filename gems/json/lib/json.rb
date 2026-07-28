# The native half first -- CRuby's loader idiom (`ext/json/lib/json.rb` reaches
# its C parser and generator the same way, through `json/ext`).
require "json.so"

module JSON
  # CRuby's hierarchy (ext/json/lib/json/common.rb): JSONError < StandardError,
  # with ParserError and GeneratorError under it. Defined here in Ruby rather
  # than in the native half because a feature-gated native class cannot
  # register a constructible exception -- see gems/strscan/lib/strscan.rb.
  class JSONError < StandardError; end
  class ParserError < JSONError; end
  class GeneratorError < JSONError; end
end

# `require "json"` gives every object a `#to_json`. CRuby installs these from
# the generator (`json/ext/generator`), which defines one per type it can encode
# directly, plus the `Object` catch-all from `json/common.rb`. The argument is
# the generator state CRuby threads through; nothing here needs it, but the
# parameter has to be accepted -- `obj.to_json(state)` is how a container asks
# its elements to encode themselves.
class Object
  # Anything with no JSON representation of its own encodes as its `to_s`, AS A
  # JSON STRING (`Object.new.to_json` is `"\"#<Object:0x...>\""`, not an
  # object) -- `JSON.generate(self)` would raise here instead.
  def to_json(*) = to_s.to_json
end

class Hash
  def to_json(*) = JSON.generate(self)
end

class Array
  def to_json(*) = JSON.generate(self)
end

class String
  def to_json(*) = JSON.generate(self)
end

class Integer
  def to_json(*) = JSON.generate(self)
end

class Float
  def to_json(*) = JSON.generate(self)
end

# A Symbol encodes as its name, like a String -- the `json/add/symbol` form
# (`{"json_class":"Symbol",...}`) is opt-in and not loaded by a plain require.
class Symbol
  def to_json(*) = JSON.generate(self)
end

class NilClass
  def to_json(*) = "null"
end

class TrueClass
  def to_json(*) = "true"
end

class FalseClass
  def to_json(*) = "false"
end

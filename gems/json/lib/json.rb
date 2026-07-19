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

# Whether a superclass NAME is a compile-time class or a constant holding one
# minted at run time is a question about the site that spells it. Asking the
# whole arena "is there a `class Error` anywhere" answers for every other site
# too: citrus writes `module Citrus; class Error < StandardError`, toml-rb
# writes `module TomlRB; Error = Class.new(StandardError)`, and with both
# compiled in the second one's subclass was put on the static path, where
# nothing defines `TomlRB::Error` at all.

module Citrus
  class Error < StandardError; end
  class ParseError < Error; end
end

module TomlRB
  Error = Class.new(StandardError)
  ParseError = Class.new(Error)

  class ValueOverwriteError < Error
    def initialize(key)
      super("Key #{key.inspect} is defined more than once")
    end
  end
end

p TomlRB::ValueOverwriteError.new("k").message
p TomlRB::ValueOverwriteError.superclass.equal?(TomlRB::Error)
p TomlRB::ParseError.superclass.equal?(TomlRB::Error)
p Citrus::ParseError.superclass.equal?(Citrus::Error)
p TomlRB::Error.superclass

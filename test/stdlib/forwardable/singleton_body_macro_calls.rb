# A `class << self` body whose statements are CALLS: they run with the singleton
# class as self, so the macros they invoke define class methods of the enclosing
# class. The `extend Forwardable` + `def_delegators` pair is the corpus's most
# common shape; `public`/`private` over a name list is fileutils'.

require "forwardable"

class Config
  def initialize
    @timeout = 30
    @retries = 2
  end

  attr_reader :timeout, :retries
end

class Client
  SETTINGS = Config.new

  class << self
    extend Forwardable

    def_delegators :settings, :timeout, :retries

    def settings
      SETTINGS
    end

    def a
      "a"
    end

    def b
      "b"
    end

    private :b
  end
end

puts Client.timeout
puts Client.retries
puts Client.a
puts Client.respond_to?(:b)
puts Client.send(:b)
puts Client.singleton_class.private_method_defined?(:b)
__END__
30
2
a
false
b
true

# `class SecretKeys::Encryptor` where `SecretKeys` itself came from
# `class SecretKeys < DelegateClass(Hash)` -- a namespace no compile-time class
# backs, because its own superclass is only computed when the program RUNS.
#
# The static path can only report `unknown class/module SecretKeys`, so the
# nested definition is minted at runtime too: the constant lands inside the
# parent that does exist by then. `Kanshi = Class.new` and
# `class FA < Struct.new(...)` reach the same place by different routes.
require "delegate"

class SecretKeys < DelegateClass(Hash)
  def initialize(h)
    super
  end
end

# A body carrying everything the runtime spelling has to re-say: a constant,
# a class method, an `include`, a visibility directive and an `alias`.
module Tagged
  def tagged = "<#{tag}>"
end

class SecretKeys::Encryptor
  PREFIX = "$AES$:"
  include Tagged

  def self.build = new
  def tag = "enc"
  def encrypt(s) = PREFIX + hidden(s)
  alias cipher encrypt

  private

  def hidden(s) = s.reverse
end

module SecretKeys::Util
  def self.on? = true
end

p SecretKeys::Encryptor::PREFIX
p SecretKeys::Encryptor.build.tagged
p SecretKeys::Encryptor.new.encrypt("abc")
p SecretKeys::Encryptor.new.cipher("abc")
p SecretKeys::Encryptor.name
p SecretKeys::Util.on?
begin
  SecretKeys::Encryptor.new.hidden("x")
rescue NoMethodError => e
  puts e.message
end
p SecretKeys.new({a: 1})[:a]

# The `Class.new` route into the same shape.
Kanshi = Class.new

class Kanshi::Collector
  def self.run = "collected"
end

p Kanshi::Collector.run
p Kanshi::Collector.name

# And the `Struct.new` one, with a superclass of its own on the nested class.
class FA < Struct.new(:a, :b)
end

class FA::Agg < Hash
  def initialize
    super
    self.default = 0
  end
end

p FA.new(1, 2).a
p FA::Agg.new[:missing]
p FA::Agg.superclass

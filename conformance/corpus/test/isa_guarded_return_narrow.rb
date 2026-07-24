module Addressable
  class URI
    attr_reader :host
    def self.parse(uri)
      return uri if uri.is_a?(URI)
      new(uri.to_s)
    end
    def initialize(host)
      @host = host
    end
    def join(other)
      URI.new(URI.parse(other.to_s).host)
    end
  end
end
j = Addressable::URI.parse("h").join("c")
puts j.host

# a genuinely mixed call profile still works (param widens to poly)
class Wrapper
  attr_reader :v
  def self.wrap(x)
    return x if x.is_a?(Wrapper)
    new(x)
  end
  def initialize(v) = @v = v
end
a = Wrapper.wrap("s")
b = Wrapper.wrap(a)
p a.v
p b.v

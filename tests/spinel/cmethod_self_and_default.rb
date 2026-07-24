# Bare `self` inside a class method is the Class object; a default-parameter
# expression referencing self evaluates in the CALLEE's context even when
# the default is materialized at a caller in another class.
class Wrap
  def initialize(model)
    @model = model
  end

  def read(key)
    "#{@model}:#{key}"
  end
end

class Keystore
  def self.value_for(key, rel = Wrap.new(self))
    rel.read(key)
  end
end

class Traffic
  def self.intensity
    Keystore.value_for("k")
  end
end

class K
  def self.who
    self
  end
end

puts Traffic.intensity
p K.who

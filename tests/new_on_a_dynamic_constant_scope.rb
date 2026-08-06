# faraday's `self.class::Handler.new(...)`: the receiver spells a constant path,
# but its scope is only known at run time, so the class comes from the receiver's
# own class -- a subclass overrides which one by redefining the constant.

class Builder
  class Handler
    def initialize(name)
      @name = name
    end

    def to_s
      "handler:#{@name}"
    end
  end

  def build(name)
    self.class::Handler.new(name)
  end

  def limit
    self.class::MAX
  end

  MAX = 10
end

class LoudBuilder < Builder
  class Handler
    def initialize(name)
      @name = name
    end

    def to_s
      "LOUD:#{@name}"
    end
  end

  MAX = 99
end

puts Builder.new.build("a")
puts LoudBuilder.new.build("b")
puts Builder.new.limit
puts LoudBuilder.new.limit

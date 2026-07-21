# A yield-inlined method assigning its yield to a LOCAL, called with blocks of
# divergent value types: the local must be a boxed carrier so each site boxes
# its own value (it settled the local from the first site otherwise).
module NS
  class Base
    def self.ping = "ping"
  end
end
module NS
  class Base
    def self.transaction
      result = yield
      result
    end
  end
end
a = NS::Base.transaction { "str" }
b = NS::Base.transaction { 42 }
puts a
puts b

# a body that further consumes the yielded local
class W
  def self.wrap
    r = yield
    "[#{r.inspect}]"
  end
end
p W.wrap { "s" }
p W.wrap { 7 }
p W.wrap { [1, 2] }

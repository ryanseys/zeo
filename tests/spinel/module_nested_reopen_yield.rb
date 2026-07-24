module NS
  class Base
    def self.ping
      "ping"
    end
  end
end

module NS
  class Base
    def self.transaction
      yield
    end
    def self.compute
      yield
    end
  end
end

puts NS::Base.ping
puts(NS::Base.transaction { "inside" })
v = NS::Base.transaction { "assigned" }
puts v
n = NS::Base.compute { 41 + 1 }
p n

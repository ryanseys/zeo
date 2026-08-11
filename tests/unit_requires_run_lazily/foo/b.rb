puts "b body runs"
module Outer
  module Foo
    class B
      def self.origin = "b loaded"
    end
  end
end

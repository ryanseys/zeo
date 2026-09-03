require_relative "b"
puts "a body runs"
module Outer
  module Foo
    class A
      def self.combined = "a sees #{B.origin}"
    end
  end
end

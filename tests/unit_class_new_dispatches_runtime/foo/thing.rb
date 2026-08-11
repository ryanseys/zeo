module Outer
  module Foo
    class Thing
      def initialize(a, b = 10)
        @val = a * b
      end
      attr_reader :val
    end
  end
end

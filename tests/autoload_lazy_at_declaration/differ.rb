module Outer
  module Foo
    class Differ
      ORIGIN = Outer::Foo.tag("differ")
      def self.origin = ORIGIN
    end
  end
end

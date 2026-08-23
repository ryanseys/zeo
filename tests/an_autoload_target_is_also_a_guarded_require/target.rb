puts "target.rb ran"
module Outer
  class Target
    SPLIT = :split
    class << self
      def read = SPLIT
    end
  end
end

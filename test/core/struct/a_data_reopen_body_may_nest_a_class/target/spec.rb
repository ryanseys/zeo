module Pkg3
  class Target
    class Spec
      def initialize(name) = (@name = name; @build = nil)
      def build(&block) = (@build = block)
      def fin!
        raise ArgumentError, "needs a build block" if @build.nil?
        Pkg3::Target.new(name: @name, build_block: @build)
      end
    end
  end
end

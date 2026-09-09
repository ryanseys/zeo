puts "  ns.rb body ran"
module NS
  LOADED = true
  module Inner
    class Deep
      def self.hi = "deep"
    end
  end
end

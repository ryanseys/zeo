# The GUARDED reopen, in a unit walked before the unguarded bodies -- the
# order `bundler/rubygems_ext.rb` and `rubygems/basic_specification.rb` are
# walked in, which is what made the ordinals disagree.
module Probe
  class Unit
    [:tag].each { |n| attr_accessor(n) }

    if Probe.cpu == "universal"
      ONLY_IF_UNIVERSAL = "universal"

      def extensions_dir
        ONLY_IF_UNIVERSAL
      end

      def self.channel
        ONLY_IF_UNIVERSAL
      end
    end
  end
end

require_relative "real"

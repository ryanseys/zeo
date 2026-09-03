# The unguarded bodies, in a SECOND unit, reached after the guarded reopen
# above -- rubygems' `basic_specification.rb` half of the pair.
module Probe
  class Unit
    def extensions_dir
      "real"
    end

    def self.channel
      "real class method"
    end
  end
end

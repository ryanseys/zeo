# The `if defined?(Const) ... module M ... end` idiom (singleton's
# `if defined?(Ractor)` tail): a decidably-TRUE guard's branch is spliced
# through the top-level walk (its module registers, its methods resolve),
# a decidably-FALSE guard's branch is dropped entirely -- exactly the
# branch real Ruby would or wouldn't execute there. Nesting folds too.

if defined?(String)
  module Kept
    def self.tag
      "kept"
    end
  end
  if defined?(Integer)
    module KeptNested
      def self.tag
        "nested"
      end
    end
  end
end
if defined?(NoSuchConstantAnywhere)
  module Dropped
    def self.tag
      "dropped"
    end
  end
end
puts Kept.tag
puts KeptNested.tag
puts defined?(Dropped).inspect
__END__
kept
nested
nil

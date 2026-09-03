# A `refine` written inside a class body that a RUNTIME-undecidable `if`
# reopened. power_assert does exactly this -- every refinement it owns lives
# inside `if PowerAssert.configuration._redefinition; module PowerAssert`,
# and test-unit inherits the failure through it.
#
# `try_conditional_reopen` rewrites `if C; module M; BODY; end; end` into
# `module M; if C; BODY; end; end`, so the class-body walk sees an `If`, not
# the `refine` inside it. Which class a refinement refines is a compile-time
# fact about SHAPE -- the same reason a nested `class`/`module` is registered
# through that same `If` -- so it is recorded even though the branch may never
# run, and the spent marker then emits nothing at its document position.

class Conf
  def self.redefinition?
    true
  end
end

module Shouty
end

if Conf.redefinition?
  module Shouty
    refine String do
      def loud
        upcase + "!"
      end
    end

    refine Integer do
      def loud
        "<#{self}>"
      end
    end
  end
end

# The refinement answers only where it is activated...
using Shouty
p "hi".loud
p 7.loud

# ...and the holder module is still not a name the target answers to.
p String.instance_methods.include?(:loud)
p String.method_defined?(:loud)

# An unrefined method on the same receiver is untouched.
p "hi".upcase

# The refinement is active inside its own block too, so a sibling `refine`
# body reaches the set by explicit receiver.
module Quiet
  refine String do
    def soft
      downcase
    end

    def whisper
      soft + "..."
    end
  end
end

using Quiet
p "LOUD".whisper
__END__
"HI!"
"<7>"
false
false
"HI"
"loud..."

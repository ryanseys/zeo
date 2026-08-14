# A method installed by `module_eval` on a STRING can reach the module's other
# methods through `self`. i18n's delegators do exactly that -- the eval'd
# `backend=` calls `writable_config`, a literal `def` two lines above it.
#
# zeo installs the eval'd method through the VM, and its self-send does not
# find the module's compiled siblings: the receiver is reported as "an instance
# of Host" where `self` is the module itself. `Host.alpha` raises
# `undefined method 'helper'` instead of answering `:helper`.
#
# This is what still blocks i18n after the mixin of such a module became a
# runtime send (tests/a_mixin_of_a_module_that_defines_methods_at_runtime.rb):
# `I18n.backend = ...` now resolves and then dies inside on
# `writable_config`.
#
# Independent of the mixin: it reproduces through `include` as well, and on the
# binary built before that change.
module Host
  module Base
    def helper = :helper

    %w(alpha).each do |name|
      module_eval <<-RUBY, __FILE__, __LINE__ + 1
        def #{name}
          helper
        end
      RUBY
    end
  end
  extend Base
end

p Host.alpha

class Direct
  include Host::Base
end
p Direct.new.alpha

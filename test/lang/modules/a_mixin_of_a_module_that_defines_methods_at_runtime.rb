# A static `include`/`extend` folds the module's methods into the target's
# table where the mixin is written, which is only sound when that table is
# COMPLETE. i18n's delegators are a string eval in a loop:
#
#   %w(locale backend default_locale ...).each do |method|
#     module_eval <<-DELEGATORS
#       def #{method} ... end
#     DELEGATORS
#   end
#   extend Base
#
# so the literal `def`s in `Base` reached `I18n` and the eval'd ones did not --
# `I18n.translate` worked while `I18n.backend` was a NoMethodError. Such a
# mixin becomes a runtime send, which installs whatever the module holds by the
# time it runs.
module Host
  module Base
    def literal = :literal

    %w(alpha beta).each do |name|
      module_eval <<-RUBY, __FILE__, __LINE__ + 1
        def #{name}
          :#{name}
        end
        def #{name}=(v)
          @#{name} = v
        end
      RUBY
    end
  end
  extend Base
end

p Host.literal
p Host.alpha
p Host.beta
p Host.respond_to?(:alpha=)
Host.alpha = 9
p Host.instance_variable_get(:@alpha)

# The INCLUDE side of the same shape.
module Carrier
  module Feature
    def plain = :plain
    %w(gamma).each do |name|
      module_eval "def #{name}; :#{name}; end", __FILE__, __LINE__
    end
  end
  class User
    include Feature
  end
end
p Carrier::User.new.plain
p Carrier::User.new.gamma

# A module with NO runtime installs keeps the static edit -- `attr_accessor`
# and a literal `define_method` both expand at compile time, so listing them
# would push nearly every module onto the slow path for nothing.
module Static
  module Plain
    attr_accessor :slot
    define_method(:made) { :made }
    def direct = :direct
  end
  class Holder
    include Plain
  end
end
h = Static::Holder.new
h.slot = 3
p [h.slot, h.made, h.direct]
__END__
:literal
:alpha
:beta
true
9
:plain
:gamma
[3, :made, :direct]

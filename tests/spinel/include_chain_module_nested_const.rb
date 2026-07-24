# `include A` brings A's nested constants/modules into the includer's
# constant lookup chain. The bug: spinel tracked `include` on
# `@cls_includes` only when it appeared in a class body, so a module
# that itself does `include X` never had X recorded anywhere. When a
# downstream class then included that module and a method body
# referenced a bare nested constant from X, the lookup walked the
# class's direct includes once and bailed (`X` wasn't a class in
# `@cls_names`, so the recursion couldn't continue).
#
# The fix tracks module-to-module includes in a parallel
# `@module_includes` array and routes const lookup through a shared
# walker that recurses into modules as well as classes.

module ActionView
  module ViewHelpers
    def self.reset_slots!
      42
    end
  end
end

module RequestDispatch
  include ActionView

  def call
    ViewHelpers.reset_slots!
  end
end

class Test
  include RequestDispatch
end

puts Test.new.call

# Multi-level chain: include of an include. Top-level constant lookup
# still finds the deeply nested module's class method.
module Outer
  module DeepHelpers
    def self.label
      "deep"
    end
  end
end

module MiddleMixin
  include Outer
end

module OuterMixin
  include MiddleMixin

  def fetch_label
    DeepHelpers.label
  end
end

class Consumer
  include OuterMixin
end

puts Consumer.new.fetch_label

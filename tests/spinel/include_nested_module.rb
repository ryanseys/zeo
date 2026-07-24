# Issue #425. A bare-receiver call to a method defined on an
# `include`d module failed when the including class was nested
# inside a module namespace:
#
#   module Ns
#     module Helper
#       def helped; "helped"; end
#     end
#     class Base
#       include Helper
#       def call; helped; end          # warning: cannot resolve
#     end
#   end
#
# Top-level usage (no `module Ns` wrapper) worked, so the
# include-resolution + bare-receiver dispatch machinery was
# intact -- the gap was just the include arg's name lookup.
#
# Root cause: `collect_class_with_prefix`'s include handlers
# (both the class-reopening branch and the fresh-class branch)
# passed the unqualified ConstantReadNode name (`Helper`) to
# `collect_module_methods_into_class`, but the module's
# registered name post-`collect_module_with_prefix` is
# `<prefix>_<name>` -- `Ns_Helper` in this case. The bare-name
# lookup walked @module_names and missed every qualified entry.
#
# Fix: new helper `resolve_include_module_name(mod_name,
# module_prefix)` tries the `<prefix>_<name>` form against
# @module_names first; falls back to the bare name for
# top-level modules so the existing top-level path stays valid.
# Both include-handler call sites in collect_class_with_prefix
# now route through it.
#
# Coverage:
#   - Canonical Rails-style shape (the repro from #425): module
#     namespace, included module also in the namespace.
#   - Cross-namespace include: top-level module included into a
#     nested class.  The qualified lookup misses, falls back to
#     the bare name, still resolves.
#   - Multiple levels of nesting via class reopening would be a
#     further extension; not covered here to keep the fix
#     minimal.

module Ns
  module Helper
    def helped
      "helped-from-Ns"
    end
  end

  class Base
    include Helper

    def call
      helped
    end
  end
end

puts Ns::Base.new.call            # helped-from-Ns

# Cross-namespace fallback: top-level module included into a
# nested class. The qualified `Ns2_Helper2` doesn't exist, so the
# resolver falls back to the bare `Helper2` which DOES exist at
# top-level.
module Helper2
  def helped2
    "helped-from-top"
  end
end

module Ns2
  class Base2
    include Helper2

    def call2
      helped2
    end
  end
end

puts Ns2::Base2.new.call2          # helped-from-top

# Regression check: same shape with no nesting still works.
module BareHelper
  def hi
    "hi"
  end
end

class BareBase
  include BareHelper

  def go
    hi
  end
end

puts BareBase.new.go               # hi

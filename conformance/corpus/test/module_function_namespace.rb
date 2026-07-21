# `module_function` in a module body installs the subsequent
# `def name` as both an instance method (for include-mixin)
# and a class method (`Mod.name`). Spinel only models the
# class-method form here.
#
# The namespace-resolution bug surfaces independently of
# `module_function`: any method body inside a module needs
# `current_lexical_scope_name` set so that bare class refs
# (`Box.new`) walk up via `resolve_const_read_name` to find
# the qualified `Outer_Box`.
#
# Both fixes ride together: register `def name` after
# `module_function` as `Mod_cls_name`, and pin
# `@current_method_name` while iterating top-level methods so
# the body's class-ref lookups peel `<Mod>_cls_<m>` correctly.

module Outer
  module Helper
    module_function
    def small;  [1, 2, 3]; end
    def medium; [4, 5, 6, 7]; end
  end

  class Conf
    def initialize(flag)
      @flag = flag
      @arr = []
      @arr << 0    # force heap layout (sp_Outer_Conf *)
    end
    attr_reader :flag
  end

  class Box
    def initialize(conf)
      @c = conf
      @arr = []
      @arr << 0
    end
    attr_reader :c
  end

  module Builder
    # `def self.X` form — the bare `Box.new` inside walks the
    # lexical scope to resolve `Outer_Box`.
    def self.make(conf)
      Box.new(conf)
    end
  end
end

# module_function dispatch.
puts Outer::Helper.small.length    # 3
puts Outer::Helper.medium.length   # 4

# Module class method body that references a sibling class
# bare-named — used to fail with "unknown type name 'sp_Box'".
b = Outer::Builder.make(Outer::Conf.new(true))
puts b.c.flag                       # true

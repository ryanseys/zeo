# `module M; def self.new(x); ...; end; end` defines a user-level
# class method, NOT a constructor (modules can't be instantiated).
# Pre-fix the call `M.new(args)` hit compile_constructor_expr's
# user-class arm and emitted `sp_box_obj(<result>, <mod_cls_id>)`,
# mis-boxing the return as an instance of the module regardless of
# the user method's declared return type. Sibling methods on the
# same module (`def self.edit`) emitted correctly via the regular
# class-method dispatch. Issue #625.

module Views
  module Articles
    def self.new(x)
      "rendered_#{x}"
    end

    def self.edit(x)
      "edited_#{x}"
    end
  end
end

puts Views::Articles.new("a")
puts Views::Articles.edit("b")

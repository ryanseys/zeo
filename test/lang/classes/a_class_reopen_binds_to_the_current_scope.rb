# A `class Name` statement looks Name up in the CURRENT cref only -- ruby's
# `vm_define_class` asks the cref's own constant table and mints there on a
# miss. It never reaches a same-named constant in an enclosing scope, so an
# outer `class Program` is invisible to the `class Program` below, which
# reopens the `Program = Data.define(:vertex)` written right beside it.
#
# Sow's `lib/sow/gl/op.rb` is this shape, and the body was attached to the
# outer class: `Sow::GL::Op::Program.new(vertex: 1)` then raised "wrong number
# of arguments (given 1, expected 0)".

module Sow
  class Program; end
  module GL
    module Op
      Program = Data.define(:vertex)
      class Program
        def kind = :program
      end
    end
  end
end
p Sow::GL::Op::Program.new(vertex: 1).kind
p Sow::GL::Op::Program.ancestors.include?(Data)
p Sow::Program.instance_methods(false)

# A bare READ inside the inner scope sees the inner constant first too: the
# cref walk asks each scope for classes AND values before moving out.
module Sow
  module GL
    module Op
      p Program
      def self.get = Program
    end
  end
end
p Sow::GL::Op.get

# The same rule for every other definition shape: a module reopens the module
# right here, `class X < Base` mints a fresh X here and leaves the outer one
# untouched, a Class.new-minted X reopens, and a subclass of the Data class
# names the nearer constant.
module Outer
  module Shared
    def self.where = :outer
  end
  class Base; end
  class Leaf; end
  Dyn = Class.new
  module Inner
    module Shared
      def self.where = :inner
    end
    class Leaf < Base
      def where = :inner
    end
    Dyn = Class.new
    class Dyn
      def where = :inner_dyn
    end
    class Fresh
      def where = :fresh
    end
  end
  Plain = Data.define(:x)
  module Inner2
    class Plain
      def where = :fresh_plain
    end
    class Sub < Plain
      def where = :sub
    end
  end
  class Made; end
  module Inner3
    Made = Data.define(:a)
    class Sub < Made
      def where = :sub_of_data
    end
  end
end
p Outer::Inner::Shared.where, Outer::Shared.where
p Outer::Inner::Leaf.new.where, Outer::Inner::Leaf.superclass, Outer::Leaf.instance_methods(false)
p Outer::Inner::Dyn.new.where, Outer::Dyn.instance_methods(false)
p Outer::Inner::Fresh.new.where
p Outer::Inner2::Plain.new.where, Outer::Plain.members
p Outer::Inner2::Sub.new.where, Outer::Inner2::Sub.superclass
p Outer::Inner3::Sub.new(a: 1).where, Outer::Inner3::Sub.ancestors.include?(Data)
__END__
:program
true
[]
Sow::GL::Op::Program
Sow::GL::Op::Program
:inner
:outer
:inner
Outer::Base
[]
:inner_dyn
[]
:fresh
:fresh_plain
[:x]
:sub
Outer::Inner2::Plain
:sub_of_data
true

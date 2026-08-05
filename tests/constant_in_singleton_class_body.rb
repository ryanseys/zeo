# A constant assigned inside `class << self` belongs to the SINGLETON class,
# not to the module the singleton is of. zeo stores it on the module.
#
#     module M
#       class << self
#         SC = 1
#       end
#     end
#     M.const_defined?(:SC, false)                   ruby false  zeo true
#     M.singleton_class.const_defined?(:SC, false)   ruby true   zeo false
#
# `Module.nesting` shows the same thing from the other side: inside
# `class << self` ruby reports `["#<Class:M>", "M"]` and zeo reports `["M"]`,
# so the singleton body is not treated as its own lexical scope at all.
#
# Reading `SC` from a method defined in that body works either way, which is
# why it goes unnoticed -- the divergence is visible only to reflection and to
# anything that asks WHERE the constant lives. Putting private helper constants
# in `class << self` to keep them off the public module is the idiom this
# breaks.

module SingConst
  class << self
    SC = :in_singleton
    def read = SC
    def nesting = Module.nesting.map(&:to_s)
  end
end

p SingConst.read
p SingConst.nesting
p SingConst.const_defined?(:SC, false)
p SingConst.singleton_class.const_defined?(:SC, false)
p SingConst.singleton_class.constants(false)

class ClsSing
  class << self
    CS = :cls_singleton
    def read = CS
  end
end
p ClsSing.read
p [ClsSing.const_defined?(:CS, false), ClsSing.singleton_class.const_defined?(:CS, false)]

# An ordinary module body puts it on the module -- already correct.
module Plain
  PC = :plain
end
p [Plain.const_defined?(:PC, false), Plain.singleton_class.const_defined?(:PC, false)]

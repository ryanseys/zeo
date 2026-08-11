# The `alias` KEYWORD in expression position aliases on the frame's DEFAULT
# DEFINEE, which is not `self`: the receiver's singleton under
# `instance_exec` (rspec's top-level DSL pairs `def shared_examples` with
# `alias shared_context shared_examples` on the RSpec module object), and
# the module itself under `class_exec`.
module M
  def self.definitions
    proc do
      def hi(x) = "hi #{x}"
      alias ho hi
    end
  end
end
module M
  instance_exec(&M.definitions)
end
p M.hi(1)
p M.ho(2)
module N; end
N.class_exec do
  def inst = "instance side"
  alias inst2 inst
end
class K2
  include N
end
p K2.new.inst2

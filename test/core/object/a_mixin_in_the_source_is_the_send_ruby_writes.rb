# A whole-program compile records `include M` as a compile-time
# ancestry EDIT and emits no statement for it. A snippet has no class
# table to edit into, so analyze must leave the marker alone and the
# emitter sends it -- receiverless, because ruby's top-level `include`
# is a private method on `main`.

module Greeter
  def hello = "hello from #{self.class}"
  def self.included(base) = puts("included into #{base}")
end
class Plain; end
src = "include Greeter"
Plain.class_eval(src)
p Plain.new.hello
p Plain.ancestors.include?(Greeter)
obj = Object.new
ext = "extend Greeter"
obj.instance_eval(ext)
p obj.hello
__END__
included into Plain
"hello from Plain"
true
"hello from Object"

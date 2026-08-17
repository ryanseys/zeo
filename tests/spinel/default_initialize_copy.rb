class Plain
  attr_accessor :v
  def initialize(v); @v = v; end
end
a = Plain.new(1)
a.send(:initialize_copy, Plain.new(5))
p a.v
p Plain.new(1).respond_to?(:initialize_copy, true)
class WithIC
  attr_accessor :w
  def initialize(w); @w = w; end
  def initialize_copy(o); super; @w = o.w * 2; end
end
b = WithIC.new(3).dup
p b.w
p WithIC.new(1).respond_to?(:initialize_copy, true)

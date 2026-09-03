class Q1; end
Q1.singleton_class.class_eval { alias_method :made, :new }
p Q1.made.class.to_s

class Q2; end
Q2.singleton_class.class_eval { alias_method :label, :name }
p Q2.label

class Q3; end
Q3.singleton_class.class_eval { alias_method :raw, :allocate }
p Q3.raw.class.to_s

class Q4; end
Q4.singleton_class.send(:alias_method, :ims, :instance_methods)
p Q4.ims(false)

class Q5; end
Q5.singleton_class.class_eval { alias_method :str, :to_s }
p Q5.str

module Q6; end
Q6.singleton_class.class_eval { alias_method :nm, :name }
p Q6.nm

class Q9
  singleton_class.alias_method :made2, :new
end
p Q9.made2.class.to_s

class QA
  def self.new = super
end
QA.singleton_class.class_eval { alias_method :m3, :new }
p QA.m3.class.to_s

module Macro
  def tag = "from the macro"
end
class QC
  extend Macro
end
QC.singleton_class.class_eval { alias_method :label, :tag }
p QC.label
__END__
"Q1"
"Q2"
"Q3"
[]
"Q5"
"Q6"
"Q9"
"QA"
"from the macro"

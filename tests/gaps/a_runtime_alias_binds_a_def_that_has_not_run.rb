class T1; end
T1.class_eval { alias_method :eql?, :== }
class T1
  def ==(other) = true
end
a, b = T1.new, T1.new
p [a == b, a.eql?(b), a.eql?(a)]

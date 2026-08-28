module MR
  def method_removed(n) = ((@s ||= []) << n)
  def s = @s
end
class CR
  def a; end
  def b; end
  remove_method :a
  extend MR
  remove_method :b
end
p CR.s

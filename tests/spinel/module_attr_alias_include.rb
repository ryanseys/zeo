module M001
  attr_accessor :x001
end
class C001
  include M001
  def initialize; @x001 = 5; end
end
p C001.new.x001
c = C001.new
c.x001 = 9
p c.x001

module M002
  def visible002; true; end
  alias_method :visible002?, :visible002
end
class C002
  include M002
end
p C002.new.visible002?

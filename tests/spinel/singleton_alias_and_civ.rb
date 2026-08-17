class C003
  @shown003 = true
  @count003 = 2
  class << self
    attr_reader :shown003
    attr_accessor :count003
    alias_method :shown003?, :shown003
  end
  def self.bump; @count003 += 1; end
end
p C003.shown003
p C003.shown003?
p C003.count003
C003.count003 = 7
p C003.count003
C003.bump
p C003.count003

class D003
  class << self
    attr_accessor :label003
  end
end
p D003.label003
D003.label003 = "hi"
p D003.label003

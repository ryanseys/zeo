# `dup`/`clone` run the copy hooks only where CRuby's road would reach code
# the program wrote. A NATIVE receiver's own initialize_copy row guards the
# direct spelling on a built receiver -- dup's fresh copy is not that, so
# Time#dup works while Time#initialize_copy on a built Time still refuses.
t = Time.utc(2026, 1, 2, 3, 4, 5)
d = t.dup
p d == t
p d.equal?(t)
begin
  t.send(:initialize_copy, d)
rescue TypeError => e
  p e.message
end

class Carrier
  attr_reader :log
  def initialize = @log = []
  def initialize_copy(other)
    @log = other.log + [:copied]
    super
  end
end

c = Carrier.new
p c.dup.log
p c.clone.log
p c.log

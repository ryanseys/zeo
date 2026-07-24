class Foo
  def initialize = @hash = {}
  def empty? = @hash.empty?
end
class Crash
  def self.fn(&) = new(&).fn
  def initialize = yield (@foo = Foo.new)
  def fn = @foo.empty?
end
p Crash.fn {}

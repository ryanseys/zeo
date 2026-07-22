class Foo
  def initialize = @value = false
  def set(*names) = @value = true
  def value = @value
end
class Crash
  def initialize = yield (@foo = Foo.new)
  def fn = @foo.value
end
module App
  def self.fn(&) = Crash.new(&).fn
end
raise unless App.fn { _1.set "foo" }
puts "ok"

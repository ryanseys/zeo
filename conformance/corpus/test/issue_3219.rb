class Crash
  attr_reader :text
  def initialize(text) = @text = text
  def value = @value ||= /<(.*)>/.match(text)[1].to_sym
end
def fn(flag) = flag ? "<foo>" : nil
raise unless Crash.new(fn(true)).value == :foo
puts "ok"

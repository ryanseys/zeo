class Sub
  attr_accessor :topic, :fd, :mode
  def initialize(topic, fd, mode)
    @topic = topic
    @fd = fd
    @mode = mode
  end
end

class App
  attr_accessor :subs
  def initialize
    @subs = [Sub.new("_", -1, 0)]
    @subs.delete_at(0)
  end

  def register(topic, fd)
    @subs << Sub.new(topic, fd, 1)
  end

  def grow_topics
    @subs[0].topic << "x" unless @subs.empty?
  end
end

a = App.new
a.register(+"news", 3)
a.grow_topics
puts a.subs[0].topic

subs = [Sub.new(+"local", 1, 0)]
subs[0].topic << "!"
subs[0].topic.upcase!
puts subs[0].topic

begin
  a.register("frozen-lit", 4)
  a.subs[1].topic << "x"
  puts "BUG: no raise"
rescue FrozenError
  puts "FrozenError"
end

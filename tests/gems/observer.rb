# observer vendored -- pure Ruby, and drb's dependency.
require "observer"

class Ticker
  include Observable

  def tick(price)
    changed
    notify_observers(Time.at(0), price)
  end
end

class Warner
  def initialize(limit) = @limit = limit
  def update(_time, price) = (puts "#{self.class}: #{price}" if price > @limit)
end

class Counter
  attr_reader :seen
  def initialize = @seen = 0
  def update(*) = @seen += 1
end

ticker = Ticker.new
counter = Counter.new
ticker.add_observer(Warner.new(100))
ticker.add_observer(counter)
p ticker.count_observers

ticker.tick(99)
ticker.tick(101)
p counter.seen

ticker.delete_observer(counter)
p ticker.count_observers
ticker.tick(102)
p counter.seen

ticker.delete_observers
p ticker.count_observers

# An observer must answer the named callback.
begin
  ticker.add_observer(Object.new)
rescue NoMethodError => e
  puts e.message
end

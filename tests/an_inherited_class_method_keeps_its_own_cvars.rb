# A class variable belongs to the class the code was WRITTEN in, never to
# the receiver that reached it -- an inherited class method still reads and
# writes its own class's storage.
class Base
  @@subs = []
  def self.subs = @@subs
  def self.note(k) = @@subs << k
end

class Late < Base
end

class Later < Late
end

Late.note("late")
Later.note("later")
p Base.subs
p Late.subs
p Later.subs

module Counter
  @@hits = 0
  def self.hit = @@hits += 1
end
Counter.hit
p Counter.hit

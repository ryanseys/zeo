p Time.method(:new).owner
p Time.singleton_methods(false).sort

class Time
  def initialize(*) = @t = "T"
  def mine = @t
end
p [Time.new.mine, Time.new.instance_variables]

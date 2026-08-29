class Time
  def initialize(*) = @t = "T"
  def mine = @t
end
p [Time.new.mine, Time.new.instance_variables]

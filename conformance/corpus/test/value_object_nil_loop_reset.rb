# A value-type object local assigned inside a block loop is reset with a
# zeroed struct, not NULL -- a method returning the object on some paths
# and (implicit) nil on others is still a value type (#3267).
class Flag
  def initialize(bool) = @bool = bool
  def bool? = @bool
end

def find_flag(name)
  return Flag.new(true) if name == "-q"
  return Flag.new(false) if name == "-n"
end

def check(argument)
  (1...argument.length).each do |idx|
    flag = find_flag("-#{argument[idx]}")
    return false unless flag&.bool?
  end
  true
end

puts check("-qn")
puts check("-qq")

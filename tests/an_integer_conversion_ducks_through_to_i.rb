p Integer(Time.at(5))
p format("%011o", Time.at(0))
p format("%d", Time.at(42))

class OnlyToI
  def to_i = 9
end
p Integer(OnlyToI.new)
p format("%d", OnlyToI.new)

class OnlyToInt
  def to_int = 7
end
p format("%d", OnlyToInt.new)

begin
  Integer(nil)
rescue TypeError => e
  puts e.message
end
begin
  format("%d", Object.new)
rescue TypeError => e
  puts e.message
end
begin
  Integer(false)
rescue TypeError => e
  puts e.message
end

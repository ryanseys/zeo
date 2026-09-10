# `day` and `hour` answer the member, not Time's, for values flowing through a
# block parameter.
Event = Data.define(:day, :hour)
events = [Event.new("Mon", 9), Event.new("Wed", 14)]
p events.map { |e| e.day }
events.each { |e| p e.hour }

Point = Struct.new(:day)
pts = [Point.new("x"), Point.new("y")]
p pts.map { |e| e.day }
__END__
["Mon", "Wed"]
9
14
["x", "y"]

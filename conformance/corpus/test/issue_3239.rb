# A Data/Struct field reader whose name collides with a Time builtin must
# dispatch to the member reader when the value flows through a poly block param.
Event = Data.define(:day, :hour)
events = [Event.new("Mon", 9), Event.new("Wed", 14)]
p events.map { |e| e.day }
events.each { |e| p e.hour }

Point = Struct.new(:day)
pts = [Point.new("x"), Point.new("y")]
p pts.map { |e| e.day }

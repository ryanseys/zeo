# An explicit `nil` endpoint IS the open end: `nil..5` and `..5` are the SAME
# range. zeo kept the two apart, so `(1..nil).min` read a bounded range and
# iterated forever, `(nil..5).first` answered nil instead of raising, and the
# pair compared unequal.
#
# `Range.new` is the same constructor under another name, and was missing.

def show(label)
  print label, ": "
  p yield
rescue Exception => e # rubocop:disable Lint/RescueException
  p [e.class, e.message]
end

RANGES = {
  "beginless-lit" => (..5),
  "nil-begin" => (nil..5),
  "endless-lit" => (1..),
  "nil-end" => (1..nil),
  "both-nil" => (nil..nil),
}.freeze

RANGES.each do |name, r|
  show("#{name} inspect") { r.inspect }
  show("#{name} endpoints") { [r.begin, r.end, r.exclude_end?] }
  show("#{name} first") { r.first }
  show("#{name} last") { r.last }
  show("#{name} size") { r.size }
  show("#{name} cover") { r.cover?(3) }
  show("#{name} min") { r.min }
end

# The nil spelling and the omitted spelling are one range.
show("eq begin") { (nil..5) == (..5) }
show("eq end") { (1..nil) == (1..) }
show("eq excl") { (1...nil) == (1...) }
show("both-nil is neither") { [(nil..nil) == (..5), (nil..nil) == (1..)] }

# `Range.new` normalizes the same way, and rejects an incomparable pair a
# literal could never build.
show("new") { Range.new(1, 5).inspect }
show("new excl") { Range.new(1, 5, true).inspect }
show("new truthy excl") { Range.new(1, 5, 1).exclude_end? }
show("new nil begin") { Range.new(nil, 5).inspect }
show("new nil end") { Range.new(1, nil).inspect }
show("new both nil") { Range.new(nil, nil).inspect }
show("new equals literal") { Range.new(1, 5) == (1..5) }
show("new strings") { Range.new("a", "e").to_a }
show("new bad pair") { Range.new(1, "a") }
show("new too few") { Range.new(1) }
show("new too many") { Range.new(1, 2, 3, 4) }

# A nil endpoint inside a pattern range folds the same way.
def classify(v)
  case v
  in ..0 then :low
  in 1.. then :high
  end
end
show("pattern") { [classify(-1), classify(3)] }

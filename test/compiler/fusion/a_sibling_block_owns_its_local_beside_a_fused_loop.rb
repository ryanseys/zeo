line = "mentions alpha and beta"
names = ["alpha", "gamma"]

# The fused loop is the LOCAL-receiver `map` (a typed nomination).
p ["typed", ["alpha", "gamma"].map { |t| re = Regexp.new(t); ->(l) { !re.match(l).nil? } }.map { |m| m.call(line) }]
p ["local", names.map { |t| re = Regexp.new(t); ->(l) { !re.match(l).nil? } }.map { |m| m.call(line) }]

# The fused loop is a literal `times`, and it runs AFTER the sibling.
p ["before", ["alpha", "gamma"].map { |t| rx = Regexp.new(t); -> { rx.source } }.map(&:call)]
2.times { |i| rx = Regexp.new("zzz"); f = -> { rx } }

# A fused body's own local never reaches the scope around it.
2.times { |i| kept = i * 10 }
p defined?(kept)

# ...but a name the scope assigned FIRST is shared, as ruby shares it.
shared = "outer"
2.times { |i| shared = "in#{i}" }
p shared
__END__
["typed", [true, false]]
["local", [true, false]]
["before", ["alpha", "gamma"]]
nil
"in1"

# `Process::Tms` is a Struct in ruby, so its own singleton carries the five
# names every generated Struct class carries. zeo had three of them; `new` was
# a NoMethodError and `inspect` came off `Module`.

p Process::Tms.singleton_methods(false).sort
p [Process::Tms.method(:new).arity, Process::Tms.method(:inspect).arity]

# `inspect` on the CLASS answers the class name -- the same string `to_s`
# answers. Only the declaring owner differs.
p Process::Tms.inspect
p Process::Tms.to_s

# `new` is `[]` under its other name. Fewer values than members leaves the
# rest nil; more is an error.
p Process::Tms.new.to_a
p Process::Tms.new(1, 2, 3, 4).to_a
p Process::Tms[1, 2, 3, 4].to_a
p Process::Tms.new(1, 2).to_a
begin
  Process::Tms.new(1, 2, 3, 4, 5)
rescue ArgumentError => e
  p e.message
end

# The members keep whatever was given, so `inspect` on an INSTANCE renders the
# struct line rather than the class name.
p Process::Tms.new(1, 2, 3, 4).inspect
p Process::Tms.members
p Process::Tms.keyword_init?

# `Process.times` still answers Floats through the same class.
t = Process.times
p [t.utime.class, t.stime.class, t.cutime.class, t.cstime.class]
p t.to_a.size
__END__
[:[], :inspect, :keyword_init?, :members, :new]
[-1, 0]
"Process::Tms"
"Process::Tms"
[nil, nil, nil, nil]
[1, 2, 3, 4]
[1, 2, 3, 4]
[1, 2, nil, nil]
"struct size differs"
"#<struct Process::Tms utime=1, stime=2, cutime=3, cstime=4>"
[:utime, :stime, :cutime, :cstime]
nil
[Float, Float, Float, Float]
4

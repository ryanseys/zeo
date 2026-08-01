# Which CLASS owns a builtin method is part of its behaviour, not a detail of
# reflection: it decides which receivers answer it. This locks the boundaries
# `conformance/builtin-arity.tsv` proved zeo had drawn in the wrong place.

require "pathname"
require "tmpdir"

def owner_of(klass, name)
  klass.instance_method(name).owner
rescue NameError
  "absent"
end

# `superclass`/`subclasses`/`inherited` are Class's, so a MODULE has none.
puts "-- Class, not Module"
p owner_of(Class, :superclass)
p owner_of(Class, :subclasses)
p owner_of(Class, :inherited)
[:superclass, :subclasses].each do |m|
  begin
    Comparable.public_send(m)
  rescue NoMethodError
    puts "Comparable.#{m}: NoMethodError"
  end
end

# `__id__` is the root's name for the identity integer; `object_id` is Kernel's.
puts "-- BasicObject, not Kernel"
o = Object.new
p owner_of(Object, :__id__)
p owner_of(Object, :object_id)
p o.__id__ == o.object_id

# The open-descriptor surface that needs a real file behind the fd is File's,
# so a pipe end does not answer it.
puts "-- File, not IO"
p owner_of(File, :flock)
p owner_of(File, :truncate)
r, w = IO.pipe
%i[size chmod chown flock lstat mtime truncate pipe?].each do |m|
  begin
    r.public_send(m)
    puts "IO##{m}: answered"
  rescue NoMethodError
    puts "IO##{m}: NoMethodError"
  rescue ArgumentError
    puts "IO##{m}: ArgumentError"
  end
end
r.close
w.close

Dir.mktmpdir do |d|
  path = File.join(d, "f")
  File.write(path, "hello world")
  File.open(path, "r+") do |f|
    p f.size
    p f.chmod(0o644)
    p f.flock(File::LOCK_EX)
    p f.flock(File::LOCK_UN)
    p f.lstat.class
    p f.mtime.class
    p f.truncate(5)
    p f.size
  end
  p File.read(path)
end

# The bound is SizedQueue's; an unbounded Queue has no `max` at all.
puts "-- SizedQueue, not Queue"
q = Thread::Queue.new
[:max, :max=].each do |m|
  begin
    m == :max ? q.max : (q.max = 1)
  rescue NoMethodError
    puts "Queue##{m}: NoMethodError"
  end
end
s = Thread::SizedQueue.new(2)
p s.max
s.max = 5
p s.max

# Names zeo used to answer that CRuby has nowhere.
puts "-- not methods at all"
{
  "Regexp#linear_time?" => -> { /a/.linear_time? },
  "Set#contain?" => -> { require("set") || Set.new([1]).contain?(1) },
  "Set#merge_new" => -> { require("set") || Set.new([1]).merge_new([2]) },
  "Pathname#to_str" => -> { Pathname.new("/tmp").to_str },
  "Pathname#mkdir_p" => -> { Pathname.new("/tmp").mkdir_p },
  "Pathname#rm_rf" => -> { Pathname.new("/tmp").rm_rf },
  "Proc#()" => -> { ->(x) { x }.public_send(:"()", 1) },
  "Struct[]" => -> { Struct[:a] },
}.each do |label, probe|
  begin
    probe.call
    puts "#{label}: answered"
  rescue NoMethodError
    puts "#{label}: NoMethodError"
  end
end

# ...while the real spellings still work.
p Regexp.linear_time?(/a*/)
p Pathname.new("/tmp").to_s
p ->(x) { x * 2 }.(3)
p Struct.new(:a)[1].a
require "set"
p Set.new([1]).include?(1)
p (Set.new([1]) | [2]).to_a.sort

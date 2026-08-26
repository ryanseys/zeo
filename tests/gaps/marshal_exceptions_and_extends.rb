# A dumped exception loads back. `marshal_allocate` has no Exception arm,
# so it reports "allocator undefined" for a class `rclass.rs` already knows
# how to allocate.
begin
  e = Marshal.load(Marshal.dump(Errno::EINTR.new))
  p e.class
  p e.message
rescue Exception => ex
  puts "errno\t#{ex.class}: #{ex.message}"
end

begin
  r = Marshal.load(Marshal.dump(RuntimeError.new("boom")))
  p [r.class, r.message]
rescue Exception => ex
  puts "runtime\t#{ex.class}: #{ex.message}"
end

# `extend`ed modules travel: CRuby writes an `e` record per module and
# re-extends on load.
module Tag
  def tagged? = true
end

o = Object.new
o.extend(Tag)
r = Marshal.load(Marshal.dump(o))
p r.singleton_class.ancestors.include?(Tag)
p r.tagged?

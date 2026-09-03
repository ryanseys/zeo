# `caller_locations` answers `Thread::Backtrace::Location` objects, not strings:
# forwardable reads `caller_locations(2, 1).first.path` to build its deprecation
# message.
def show
  loc, = caller_locations(1, 1)
  [loc.class.to_s, loc.path.end_with?("caller_locations_objects.rb"), loc.lineno,
   loc.label, loc.base_label]
end

def outer = show

class Holder
  def self.klass_method = show
  def inst = show
end

p outer
p Holder.klass_method
p Holder.new.inst

# A `start` past the top answers nil, matching `caller`.
p caller_locations(99)
p caller_locations.class

# `to_s` is exactly the string `caller` would have given for the same frame.
loc = caller_locations(0, 1).first
p loc.to_s == "#{loc.path}:#{loc.lineno}:in '#{loc.label}'"
p loc.inspect == loc.to_s.inspect
# (`path` is the script as invoked, `absolute_path` the resolved one; zeo's
# frames only ever carry the resolved form, so this holds either way.)
p loc.absolute_path.end_with?(loc.path)
p caller_locations(0, 2).size
__END__
["Thread::Backtrace::Location", true, 10, "Object#outer", "outer"]
["Thread::Backtrace::Location", true, 13, "Holder.klass_method", "klass_method"]
["Thread::Backtrace::Location", true, 14, "Holder#inst", "inst"]
nil
Array
true
true
true
1

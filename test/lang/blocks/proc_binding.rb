# `Proc#binding` answers the scope the block was WRITTEN in -- that scope's
# `self` and its locals, shared by reference, never the block's own locals
# (which do not exist until it runs). A proc with no Ruby scope behind it is
# CRuby's "C level Proc".

def make
  a = 1
  pr = proc { b = 2; a }
  pr
end

puts "-- the defining scope --"
pb = make.binding
p pb.class
p pb.local_variables
p pb.local_variable_get(:a)
p pb.receiver.class
p pb.source_location.first.end_with?("proc_binding.rb")
p pb.frozen?

puts "-- one environment, distinct objects --"
pr = proc { }
p pr.binding.equal?(pr.binding)
p pr.binding.local_variable_defined?(:pr)

puts "-- shared with the running proc --"
def counted
  c = 5
  pr = proc { c += 1 }
  pr.call
  [pr.binding.local_variable_get(:c), c]
end
p counted

puts "-- lambdas and block parameters keep theirs too --"
l = lambda { |q| q }
p l.binding.class
p l.binding.local_variables
def taking(&b) = b
p taking { 1 }.binding.local_variables

puts "-- the proc's OWN binding is a different scope --"
def two
  o = 1
  inner = proc { i = 2; binding }
  [inner.binding.local_variables, inner.call.local_variables]
end
p two

puts "-- a proc with no Ruby scope --"
begin
  :upcase.to_proc.binding
rescue ArgumentError => e
  p [e.class, e.message]
end
__END__
-- the defining scope --
Binding
[:a, :pr]
1
Object
true
false
-- one environment, distinct objects --
false
true
-- shared with the running proc --
[6, 6]
-- lambdas and block parameters keep theirs too --
Binding
[:pb, :pr, :l, :e]
[:pb, :pr, :l, :e]
-- the proc's OWN binding is a different scope --
[[:o, :inner], [:i, :o, :inner]]
-- a proc with no Ruby scope --
[ArgumentError, "Can't create Binding from C level Proc"]

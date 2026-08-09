# `autoload` registers a hook; it does not touch the file. So a target that is
# not on the load path is not an error at the declaration -- it is an error
# when the constant is READ, and most such constants never are. actionpack
# writes `autoload :Test, "rack/test"` in every program that loads
# action_dispatch, and rack-test is frequently not there.
#
# The constant is announced at declaration all the same, which is what
# `const_defined?` and `autoload?` report.
module Holder
  autoload :Absent, "definitely_no_such_feature_xyz"
  Present = 1
end

p Holder::Present
p Holder.autoload?(:Absent)
p Holder.const_defined?(:Absent)
p Holder.constants.include?(:Present)

begin
  Holder::Absent
rescue LoadError => e
  puts "LoadError: #{e.message}"
end

# Reading it again asks the same question again, and gets the same answer.
begin
  Holder.const_get(:Absent)
rescue LoadError => e
  puts "LoadError: #{e.message}"
end

puts "still running"

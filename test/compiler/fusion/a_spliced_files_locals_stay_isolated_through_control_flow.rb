# Real Ruby gives every `require`d file its own top-level local scope. zeo
# splices every file into one `Program`, so it renames each file's top-level
# locals to keep them apart. That renaming walk had a hand-written arm for
# every node kind an assignment can hide behind -- `if`, `while`, `case`,
# `begin`, a literal -- each cloning the body vector it walked. Collapsing
# them onto one descent has to keep reaching all of those positions: a name
# missed there is renamed at its assignment and NOT at its read, or the other
# way round, and the two files silently share a binding.
#
# This file assigns the same names the required one does, from inside each
# control-flow shape, and both must keep their own values.
name = "outer"
count = 0

if name == "outer"
  count = 1
end

while count < 3
  count += 1
end

case count
when 3 then tag = "three"
else tag = "other"
end

begin
  parts = ["a", "b"]
rescue StandardError
  parts = []
end

interp = "#{name}-#{count}-#{tag}"
listed = [name, count, tag].map { |v| v.to_s }

require_relative "a_spliced_files_locals_stay_isolated_through_control_flow/inner"

p [name, count, tag, parts, interp, listed]
p inner_report
__END__
["inner", 202, "two-oh-two", ["x"], "inner-202-two-oh-two", ["inner", "202", "two-oh-two"]]
["outer", 3, "three", ["a", "b"], "outer-3-three", ["outer", "3", "three"]]
:inner_ran

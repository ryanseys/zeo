# Issue #856: split with a regex that has capture groups splices
# the matched groups into the result between segments.
puts "hello world".split(/(\s+)/).inspect
# Without a capture group, behaviour unchanged.
puts "hello world".split(/\s+/).inspect
# Multiple capture groups: each captured substring is spliced
# in, in group-index order.
puts "a1b2c3d".split(/(\d)/).inspect

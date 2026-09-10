# A String method called with no argument where one is required raises
# ArgumentError naming the arity, rather than reading a missing argument as
# an empty or null one.
#
# WHAT THIS RECORDS: the very first probe below raises, and the raise is an
# ArgumentError that the `rescue TypeError` does not catch, so the program
# stops there. Everything after it is unreachable -- see the trailer. The
# later probes are kept as a record of what else is worth checking; each
# needs a file of its own before it can run.
begin
  "foo".count
  puts "BUG count: no raise"
rescue TypeError => e
  puts "count: #{e.message}"
end
p "foo".delete
p "foo".rindex(/missing/)
p "abcdabcd".rindex(/c/)
begin
  "foo".send(:<<)
  puts "no raise"
rescue FrozenError => e
  puts "send-lshift: " + e.message
end

# setbyte on a literal: WITHOUT the frozen_string_literal magic comment a
# literal is mutable (CRuby); the write copies the static bytes and rebinds
# the variable (#2029). A frozen-string-literal file or an explicit .freeze
# still raises (pinned by bundle_classd_43).
(str = "a")
begin
  str.setbyte(0, 98)
  puts "literal not frozen: " + str
rescue FrozenError => e
  puts "frozen literal: " + e.message
end
str2 = "a".dup
str2.setbyte(0, 98)
puts str2  # "b" (heap, mutates)
__END__
#@ stderr
core/string/str_method_nil_arg_no_segv.rb:11:in 'String#count': wrong number of arguments (given 0, expected 1+) (ArgumentError)
	from core/string/str_method_nil_arg_no_segv.rb:11:in '<main>'
#@ exit 1

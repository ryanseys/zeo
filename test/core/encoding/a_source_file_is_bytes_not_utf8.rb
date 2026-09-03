# Ruby source is a byte stream, not text. CRuby reads a file with the
# default UTF-8 source encoding and simply SKIPS a byte that is not valid
# UTF-8 when it sits in a comment -- only code has to decode. A required
# library whose copyright line carries a Latin-1 (c) therefore loads, and
# every definition in it is reachable.
require_relative "a_source_file_is_bytes_not_utf8/latin1_comment"

puts Greeter.new.hello
p Greeter.instance_method(:hello).arity
__END__
hi
0

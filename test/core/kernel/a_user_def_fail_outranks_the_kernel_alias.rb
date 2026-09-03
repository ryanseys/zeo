# `fail` is `Kernel#raise`'s exact synonym -- and a popular USER method
# name. Resolution order is real Ruby's: a `def fail` in the receiver's
# chain wins over the Kernel meaning, however many arguments it takes.
#
# zeo desugared every receiverless `fail` to `HirNode::Raise` at parse
# time, before method resolution existed -- riot's reporter (`def fail(
# description, message, line, file)`, four arguments) died with "wrong
# number of arguments (given 4, expected 0..3)", and took ten gems with it.
class Reporter
  def fail(description, message, line, file)
    puts "FAIL #{description}: #{message} (#{file}:#{line})"
  end

  def report
    fail("addition", "1 != 2", 3, "math.rb")
    :reported
  end
end

puts Reporter.new.report

# The Kernel meaning survives everywhere no user `fail` is in scope:
# bare re-raise semantics, the class-and-message form, and rescue.
begin
  fail "plain kernel fail"
rescue RuntimeError => e
  puts e.message
end

begin
  fail ArgumentError, "typed kernel fail"
rescue ArgumentError => e
  puts e.message
end

# ...including inside a method of a class that defines NO fail.
class Quiet
  def go
    fail "from a quiet method"
  end
end

begin
  Quiet.new.go
rescue RuntimeError => e
  puts e.message
end
__END__
FAIL addition: 1 != 2 (math.rb:3)
reported
plain kernel fail
typed kernel fail
from a quiet method

# Reopening the native exception hierarchy and the builtin modules (D3).
# The exception prelude lives natively in Rust, yet stays fully open: a reopen
# ADDS a method reaching every subclass and OVERRIDES an existing one
# everywhere, and a builtin module reopen reaches every includer.

# --- add a method to the whole exception tree ---
class Exception
  def tag
    "[#{self.class.name}]"
  end
end

# --- override a native method; it wins for every subclass ---
class StandardError
  def message
    "handled: " + to_s
  end
end

begin
  raise ArgumentError, "bad value"
rescue => e
  puts e.tag
  puts e.message
  puts e.is_a?(StandardError)
end

# CRuby stores the message in a hidden slot, not a @message ivar.
begin
  raise "plain"
rescue => e
  p e.instance_variables
  puts e.message
end

# --- reopen a builtin MODULE: reaches every includer ---
module Enumerable
  def second
    first(2).last
  end
end

puts [10, 20, 30].second
puts({ "a" => 1, "b" => 2 }.map { |_k, v| v }.second)

# --- reopen restating the real superclass ---
class String < Object
  def shout
    upcase + "!"
  end
end
puts "hi".shout

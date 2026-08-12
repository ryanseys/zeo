# The other half of `a_definition_ruby_raises_on_raises`: with nothing to
# catch it, the program runs up to the definition and then dies exactly where
# ruby dies -- same message, same backtrace line, same exit status. The
# statements ABOVE it still run and still print, which is what a compile-time
# refusal took away.
puts "before"

class Base; end
class Other; end
class Sub < Base; end
class Sub < Other
  def never = "unreachable"
end

puts "after"

# `__method__` inside a `def` written in a BLOCK. Such a def installs at
# runtime, as a method-body lambda, so the name has to be carried into the
# body: reading it off the enclosing scope answers whatever encloses the
# `def` -- nil at the top level.
def probe(label)
  result = yield
  puts "#{label}: #{result.inspect}"
end

probe("method_name") do
  def named_method
    __method__
  end
  named_method
end

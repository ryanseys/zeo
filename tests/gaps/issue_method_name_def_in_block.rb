# __method__ returns nil instead of the enclosing method's name symbol when
# that method was defined via `def` nested inside a block (e.g. a block
# passed to another method call), even though the same `def` at the top
# level resolves __method__ correctly.
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

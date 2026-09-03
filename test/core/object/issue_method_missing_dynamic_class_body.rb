# method_missing defined inside a `Class.new { ... }` block isn't wired up as
# the receiver's dispatch fallback; zeo raises NoMethodError for the unknown
# method instead of routing it to method_missing.
klass = Class.new do
  def method_missing(name, *args)
    "mm:#{name}:#{args}"
  end
end
p klass.new.foo(1, 2)
__END__
"mm:foo:[1, 2]"

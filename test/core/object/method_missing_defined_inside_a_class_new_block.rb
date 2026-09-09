# It is the receiver's dispatch fallback there as it is in a literal class
# body.
klass = Class.new do
  def method_missing(name, *args)
    "mm:#{name}:#{args}"
  end
end
p klass.new.foo(1, 2)
__END__
"mm:foo:[1, 2]"

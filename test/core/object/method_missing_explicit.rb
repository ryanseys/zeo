# method_missing called BY NAME, like any other method. The hook behaviour --
# a real miss routed through it -- has its own tests beside this one; here the
# call is explicit, so it takes the name and argument count it was handed.
class Proxy
  def initialize(label)
    @label = label
  end

  def method_missing(name, *args)
    "#{@label}:#{name}/#{args.length}"
  end
end

p = Proxy.new("px")
puts p.method_missing(:foo)
puts p.method_missing(:bar, 1, 2)
__END__
px:foo/0
px:bar/2

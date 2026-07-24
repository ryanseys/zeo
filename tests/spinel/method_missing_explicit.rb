# method_missing is not honored as a missing-method hook (spinel resolves every
# call statically), but a class may still define and call it explicitly like any
# other method. Defining it emits a compile-time warning to stderr.
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

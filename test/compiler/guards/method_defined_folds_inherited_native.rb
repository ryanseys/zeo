# `method_defined?` / `respond_to?` guards must see native methods a user
# class inherits from Object/Kernel -- rspec-core's 1.8.7 shim gates a
# self-recursive `singleton_class` def on exactly this probe, so a confident
# false here bakes the shim in and it recurses forever.
class Foo
  unless method_defined?(:singleton_class)
    def singleton_class
      "shim"
    end
  end
  if method_defined?(:object_id)
    def native_seen
      "native seen"
    end
  end
end

f = Foo.new
p f.singleton_class.equal?(f.singleton_class)
p f.native_seen
p f.respond_to?(:itself)
p Foo.method_defined?(:no_such_method)
__END__
true
"native seen"
true
false

# `.new` on a class whose `initialize` lives in a lazily-loaded unit is a
# call site like any other: the name de-optimizes to the dynamic path,
# which resolves the REAL initialize (the static resolution fell through to
# BasicObject's zero-arity one and baked an arity error -- prism's
# ParseResult inside rspec-support was the case).
module Outer
  module Foo
    def self.install(lib, &require_relative)
      name = "helper_#{lib}"
      (class << self; self; end).__send__(:define_method, name) do |f|
        require_relative.call("unit_class_new_dispatches_runtime/#{lib}/#{f}")
      end
    end
    install(:foo) { |f| require_relative(f) }
    helper_foo "thing"
  end
end
t = Outer::Foo::Thing.new(5)
p t.val
p Outer::Foo::Thing.new(7, 2).val

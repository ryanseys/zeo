# A require INSIDE a lazily-loaded unit runs when the unit body executes --
# CRuby's order -- rather than being woven inline into whichever unit the
# sweep reached first (the cross-unit dedup hazard rbconfig hit inside
# rspec-support's ruby_features.rb). `a.rb` requires `b.rb` at its top, so
# loading `a` must print b's side effect first, and only at load time.
module Outer
  module Foo
    def self.install(lib, &require_relative)
      name = "helper_#{lib}"
      (class << self; self; end).__send__(:define_method, name) do |f|
        require_relative.call("unit_requires_run_lazily/#{lib}/#{f}")
      end
    end
    install(:foo) { |f| require_relative(f) }
  end
end
puts "before load"
Outer::Foo.helper_foo "a"
puts Outer::Foo::A.combined

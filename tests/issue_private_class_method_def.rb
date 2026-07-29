# `private_class_method def helper ...` under a `module_function` directive
# registers the def: `private_class_method` takes the def's symbol as an
# argument, and the def itself defines the method as usual. zeo does not model
# class-method VISIBILITY (tests/gaps/class_method_visibility.rb), so the
# helper stays callable -- what this pins is that it EXISTS.
module M
  module_function

  def pub
    helper * 2
  end

  private_class_method def helper
    21
  end
end

p M.pub

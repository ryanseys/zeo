# `private_class_method def helper` registers the method. zeo does not model
# class-method visibility, so what this pins is that the method exists.
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
__END__
42

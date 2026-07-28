# `private_class_method def helper ...` under a `module_function` directive
# should still register the def (private_class_method takes the def's
# symbol as an argument; the def itself defines the method as usual). zeo's
# class-body walk treats the whole statement as a plain method call and
# never registers the def, so the method is simply missing.
#
# Until this works, the vendored bigdecimal Ruby half (gems/bigdecimal)
# carries a patch stripping `private_class_method` off its internal
# helpers -- see the `zeo:` comments there; drop them when promoting this.
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

# Before this fix, `analyze::collect_ivars`/`locals::track_extra`/
# `hoisting`'s per-method scans never walked into a `Params::optional`/
# `keywords` default expression -- an ivar referenced ONLY there (never
# in the method's own body) would be missing from the generated
# struct's fields entirely, a "no such field" codegen error. Also
# exercises the keyword-optional case (`y:`) in the same call.

class Greeter
  def initialize
    @default_name = "World"
  end
  def greet(x = @default_name, y: @default_name)
    "hi #{x} and #{y}"
  end
end

puts Greeter.new.greet
__END__
hi World and World

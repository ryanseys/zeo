# `undef_method` under a guard zeo cannot decide is a RUNTIME fact, so the
# name goes on the class's `runtime_undefs` and codegen stops emitting a
# direct call for it. The walk that collects those names had no stops, so a
# nested class's undef was credited to the ENCLOSING class too -- which only
# de-optimizes a name the outer class never undefs, but it de-optimizes it in
# every program that nests this way.
class Outer
  def keep
    "outer-keep"
  end

  class Inner
    def keep
      "inner-keep"
    end
    undef_method :keep if 1 == 1
  end
end

p Outer.new.keep
begin
  p Outer::Inner.new.keep
rescue NoMethodError
  p :inner_undefined
end
__END__
"outer-keep"
:inner_undefined

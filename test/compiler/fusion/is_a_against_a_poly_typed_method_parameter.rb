# A method PARAMETER is always `TyKind::Poly` (zeo never infers a
# param's type from its call sites) -- this exercises the runtime
# `zeo_rt::is_a` fallback (via the new universal `RubyValue::class_id`)
# rather than the static ancestors-constant-fold path.

class Checker
  def check(v)
    v.is_a?(Integer)
  end
end
c = Checker.new
puts c.check(5)
puts c.check("hi")
puts c.check([1, 2])
__END__
true
false
false

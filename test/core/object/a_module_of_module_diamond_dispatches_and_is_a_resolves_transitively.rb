# The exact shape zeo's OWN reflection gets wrong (verified by
# running a repro against zeo's own binary): `D` included via two
# separate paths (`B`/`C`, both including `D`) must be deduped to a
# single shared position, and `is_a?(D)` must resolve `true` even
# though `D` was never included DIRECTLY by `A`.

module D
  def who
    "D"
  end
end
module B
  include D
end
module C
  include D
end
class A
  include B
  include C
end
class Unrelated
end
puts A.new.who
puts A.new.is_a?(D)
puts A.new.is_a?(B)
puts A.new.is_a?(Unrelated)
__END__
D
true
true
false

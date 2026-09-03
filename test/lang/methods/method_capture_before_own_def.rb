# `K.method(:name)` captured BEFORE `def self.name` in document order binds
# the INHERITED entry -- CRuby resolves at capture time, so the later
# override must not be found by the capture (rspec-support's
# `NEW_MUTEX_METHOD = Mutex.method(:new)` / `def self.new` delegation pair;
# a by-name capture recurses forever).
class Base
  def self.make
    "base-make"
  end
end

class Sub < Base
  CAP = Sub.method(:make)
  def self.make
    "sub-wraps(#{CAP.call})"
  end
end

p Sub.make
p Sub.method(:make).call
__END__
"sub-wraps(base-make)"
"sub-wraps(base-make)"

# `undef` inside a reopened `class << self` retires the class method WHERE IT
# STANDS. Recording it as a compile-time fact applied it from program start, so
# a call written between the two bodies raised.
class Gone
  def self.away = :here
  def self.stays = :stays
end

p Gone.away
p Gone.respond_to?(:away)

class Gone
  class << self
    undef away
  end
end

begin
  Gone.away
rescue NoMethodError => e
  p e.class
end
p Gone.respond_to?(:away)
p Gone.singleton_class.method_defined?(:away)
# Its sibling is untouched.
p Gone.stays

# An undef written in the FIRST body still applies from the start.
class Never
  def self.a = :a
  class << self
    undef a
  end
end
begin
  Never.a
rescue NoMethodError => e
  p [:never, e.class]
end

# Two names at once, and a subclass sees the parent's retirement.
class Multi
  def self.x = :x
  def self.y = :y
end
class Sub2 < Multi; end
p Sub2.x
class Multi
  class << self
    undef x, y
  end
end
begin; Multi.x; rescue NoMethodError => e; p [:multi_x, e.class]; end
begin; Multi.y; rescue NoMethodError => e; p [:multi_y, e.class]; end
begin; Sub2.x; rescue NoMethodError => e; p [:sub_x, e.class]; end
__END__
:here
true
NoMethodError
false
false
:stays
[:never, NoMethodError]
:x
[:multi_x, NoMethodError]
[:multi_y, NoMethodError]
[:sub_x, NoMethodError]

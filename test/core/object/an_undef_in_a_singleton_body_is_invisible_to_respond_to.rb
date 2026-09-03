class Foo
  class << self
    undef_method :new
  end
end
p Foo.respond_to?(:new)
begin
  Foo.new
rescue NoMethodError => e
  p e.class
end

class Bar; end
class << Bar
  undef_method :new
end
p Bar.respond_to?(:new)
__END__
false
NoMethodError
false

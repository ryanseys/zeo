class Base
  def self.create
    :inherited
  end
end

class Sub < Base
  p methods.include?(:create)
  p Sub.respond_to?(:create)
  p respond_to?(:create)
  p Sub.create
  def self.create
    :own
  end unless respond_to? :create
end
p Sub.create

class Wide < Base
  def self.create
    :fires
  end unless $PROGRAM_NAME.empty?
end
p Wide.create
__END__
true
true
true
:inherited
:inherited
:fires

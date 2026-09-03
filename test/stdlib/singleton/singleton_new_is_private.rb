require "singleton"

class Only
  include Singleton
end

p Only.instance.equal?(Only.instance)
begin
  Only.new
rescue NoMethodError => e
  p e.class
end
p Only.respond_to?(:new)
__END__
true
NoMethodError
false

# define_method inside instance_exec: CRuby raises NoMethodError (it is a
# private method of Module, and the receiver is a plain instance).
class Box
  def initialize(v)
    @v = v
  end
end

b = Box.new(5)
begin
  b.instance_exec { define_method(:greet) { 1 } }
rescue NoMethodError => e
  puts "rescued: #{e.message}"
end

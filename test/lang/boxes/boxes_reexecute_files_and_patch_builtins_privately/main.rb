box = Ruby::Box.new
box.require_relative "blank"
box2 = Ruby::Box.new
box2.require_relative "blank"
p box::Foo.blank_one?
begin
  "foo".blank?
rescue NoMethodError => e
  puts "main: #{e.message}"
end
box::Counter.bump
box::Counter.bump
box2::Counter.bump
p box::Counter.count
p box2::Counter.count

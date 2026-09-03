box = Ruby::Box.new
box.require_relative "thrower"
begin
  box::Thrower.go
rescue StandardError => e
  puts "rescued: #{e.message} (#{e.class})"
end

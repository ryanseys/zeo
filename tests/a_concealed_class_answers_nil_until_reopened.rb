def probe? = false
if probe?
  class Ghost
    def x = 1
  end
end
if defined?(Ghost)
  puts "have ghost"
else
  puts "no ghost"
end
class Ghost
  def y = 2
end
puts defined?(Ghost)
puts Ghost.new.y
begin
  Ghost.new.x
rescue NoMethodError => e
  puts "NoMethodError"
end

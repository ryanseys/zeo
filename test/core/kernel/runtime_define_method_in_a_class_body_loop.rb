# A class-body `each` loop whose `define_method` block captures the loop
# variable installs each method into the runtime overlay, reachable on
# every instance and by `respond_to?`.

class Robot
  [:beep, :boop].each do |sound|
    define_method(sound) { "#{sound}!" }
  end
end
r = Robot.new
puts r.beep
puts r.boop
puts r.respond_to?(:beep)
puts r.respond_to?(:whir)
__END__
beep!
boop!
true
false

# puts/print/interpolation of an object whose to_s unions a String with
# another branch type (returns a boxed sp_RbVal) must route through
# sp_poly_to_s instead of a pointer cast (#3266).
class Crash
  def initialize(flag) = @flag = flag
  def to_s = @flag ? "ok" : nil
end

puts Crash.new(true)
print Crash.new(true), "\n"
x = Crash.new(true)
puts "val=#{x}"

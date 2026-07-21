# User subclasses of built-in exception classes should also lower
# to first-class sp_Exception * with cls_name = subclass name.
class MyError < StandardError
  def initialize(msg = "default msg")
    super(msg)
  end
end

e = MyError.new("hello")
puts e.message
puts e.class

e2 = MyError.new
puts e2.message
puts e2.class

# is_a? walks the parent chain.
puts e.is_a?(MyError)
puts e.is_a?(StandardError)
puts e.is_a?(RuntimeError)

# raise + rescue with user subclass
begin
  raise MyError, "raised"
rescue MyError => caught
  puts "caught: #{caught.message}"
end

# rescue by parent matches subclass
begin
  raise MyError, "via parent"
rescue StandardError => parent_caught
  puts "parent caught: #{parent_caught.message}"
  puts "parent caught cls: #{parent_caught.class}"
end

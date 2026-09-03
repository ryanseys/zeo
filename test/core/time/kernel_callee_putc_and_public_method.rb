def who = [__method__, __callee__]
p who
putc 72
putc "i"
putc "\n"
p putc(321).class
puts
p putc("BC")
puts
class Widget
  def render = "drawn"
  private
  def secret = 42
end
w = Widget.new
p w.public_method(:render).call
begin
  w.public_method(:secret)
rescue NameError => e
  puts e.message
end
__END__
[:who, :who]
Hi
AInteger

B"BC"

"drawn"
method 'secret' for class 'Widget' is private

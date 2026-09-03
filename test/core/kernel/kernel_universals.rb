# __callee__ / __method__ (identical without aliases)
def who
  [__method__, __callee__]
end
p who

# putc: Integer writes the low byte, String writes the first character;
# both return their argument.
putc 72
putc 105
putc "!"
putc "\n"
p putc(321).class   # 321 & 0xff = 65 -> "A"
puts
p putc("BC")        # writes "B", returns "BC"
puts

# public_method binds public methods, refuses private ones
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
begin
  w.public_method(:missing)
rescue NameError => e
  puts e.message
end
__END__
[:who, :who]
Hi!
AInteger

B"BC"

"drawn"
method 'secret' for class 'Widget' is private
undefined method 'missing' for class 'Widget'

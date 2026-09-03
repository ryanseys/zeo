FOO = 42
class Widget; end
puts eval("FOO".dup)
puts eval("Widget".dup)
__END__
42
Widget

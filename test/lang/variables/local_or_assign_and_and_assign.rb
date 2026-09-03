x = nil
x ||= 5
puts x
y = 1
y &&= 2
puts y
__END__
5
2

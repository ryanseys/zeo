# ruby 4.0 loads `pathname.so` before the first line, so the class and
# `Kernel#Pathname` are there with no require at all.

puts Pathname.new("/x")
puts Pathname("/y").class
__END__
/x
Pathname

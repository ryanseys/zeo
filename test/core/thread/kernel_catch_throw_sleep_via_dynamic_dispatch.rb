# `send`/`&:` force dynamic dispatch through the Kernel table rather than
# the static codegen fast path.

p send(:catch, :done) { throw :done, 42 }
p [1, 2, 3].map { |x| catch(:skip) { throw :skip, -1 if x == 2; x } }
send(:sleep, 0)
puts "slept"
__END__
42
[1, -1, 3]
slept

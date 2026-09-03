b = IO::Buffer.new(8)
p b.size
__END__
8
#@ stderr
core/io/io_buffer_experimental_warning.rb:1: warning: IO::Buffer is experimental and both the Ruby and C interface may change in the future!

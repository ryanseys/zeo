# Reading a write-only IO (or writing a read-only one) is an IOError ("not
# opened for reading"/"not opened for writing") -- ruby checks the mode
# before touching the fd. zeo lets the syscall fail and raises
# Errno::EBADF. (Found by the 2026-08-24 probe sweep.)
begin
  File.open("/dev/null", "w") { |f| f.read }
rescue IOError => e
  puts "#{e.class}: #{e.message}"
end
begin
  File.open("/dev/null", "r") { |f| f.write("x") }
rescue IOError => e
  puts "#{e.class}: #{e.message}"
end
__END__
IOError: not opened for reading
IOError: not opened for writing

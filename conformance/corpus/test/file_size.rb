# File.size(path): byte count (stat-based, so embedded NUL bytes are
# counted, unlike a strlen); a missing path raises Errno::ENOENT, matching
# MRI and the other stat/open-based File class methods.
filename = "test_size_temp.bin"
begin
  File.write(filename, "ABC")
  puts File.size(filename)
  File.write(filename, [65, 0, 66, 0, 67].pack("C*"))   # 5 bytes incl 2 NULs
  puts File.size(filename)
  begin
    File.size("definitely_missing_size_test")
    puts "no-raise"
  rescue => e
    puts e.class
  end
ensure
  File.delete(filename) if File.exist?(filename)
end

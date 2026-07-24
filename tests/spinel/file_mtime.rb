filename = "test_mtime_temp.txt"
File.write(filename, "mtime test")
begin
  t = File.mtime(filename)
  puts t.class
  # Year should be an integer
  puts t.year.class
ensure
  File.delete(filename)
end

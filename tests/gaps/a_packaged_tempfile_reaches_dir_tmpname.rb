require "tempfile"
Tempfile.create("pkggap") do |f|
  f.write("ok")
  f.rewind
  puts f.read
end

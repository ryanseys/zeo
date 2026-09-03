require "tempfile"
Tempfile.create("readable") do |f|
  f.write("ok")
  f.rewind
  puts f.read
end
__END__
ok

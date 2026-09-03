# The spliced road matches ruby; the PACKAGED road (compiled against
# packaged gems rather than spliced sources) is the one that diverges.
require "tempfile"
Tempfile.create("pkggap") do |f|
  f.write("ok")
  f.rewind
  puts f.read
end
__END__
ok

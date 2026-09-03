require "tmpdir"

Dir.mktmpdir do |d|
  f = File.join(d, "o.txt")
  File.write(f, "l1\nl2\n")
  p File.readlines(f, 2).first
  p File.readlines(f).first
end
__END__
"l1"
"l1\n"

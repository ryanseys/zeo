require "tmpdir"

Dir.mktmpdir do |d|
  Dir.mkdir(File.join(d, "sub"))
  Dir.chdir(d) { p Dir.glob("**/").sort }
end
__END__
["sub/"]

# `Dir` listing: `entries` includes `.`/`..`, `children` does not.

dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_list_#{Process.pid}")
Dir.mkdir(dir)
begin
  File.write(File.join(dir, "a.rb"), "")
  File.write(File.join(dir, "b.txt"), "")
  Dir.mkdir(File.join(dir, "sub"))

  p Dir.children(dir).sort
  p Dir.entries(dir).sort
  p Dir.exist?(dir)
  p Dir.exist?(File.join(dir, "a.rb"))
  p Dir.exist?("/definitely/not/here")
  p Dir.empty?(File.join(dir, "sub"))
  p Dir.pwd.start_with?("/")

  Dir.rmdir(File.join(dir, "sub"))
  File.delete(File.join(dir, "a.rb"), File.join(dir, "b.txt"))
ensure
  Dir.rmdir(dir)
end
__END__
["a.rb", "b.txt", "sub"]
[".", "..", "a.rb", "b.txt", "sub"]
true
false
false
true
true

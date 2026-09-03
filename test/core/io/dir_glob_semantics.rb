# `Dir.glob`: `*` within a segment, `**` across them, `?`, `{a,b}`
# alternation, and Ruby's hidden-file rule (a leading `.` is invisible to a
# wildcard).

dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_glob_#{Process.pid}")
Dir.mkdir(dir)
begin
  Dir.chdir(dir) do
    Dir.mkdir("sub")
    File.write("x.rb", "")
    File.write("y.txt", "")
    File.write(".hidden", "")
    File.write("sub/z.rb", "")

    p Dir.glob("*.rb").sort
    p Dir.glob("**/*.rb").sort
    p Dir.glob("sub/*.rb")
    p Dir.glob("*.{rb,txt}").sort
    p Dir.glob("?.rb")
    p Dir["*.rb"]
    p Dir.glob("*").sort
    p Dir.glob("*").include?(".hidden")
    p Dir.glob(".*").include?(".hidden")

    File.delete("x.rb", "y.txt", ".hidden", "sub/z.rb")
    Dir.rmdir("sub")
  end
ensure
  Dir.rmdir(dir)
end
__END__
["x.rb"]
["sub/z.rb", "x.rb"]
["sub/z.rb"]
["x.rb", "y.txt"]
["x.rb"]
["x.rb"]
["sub", "x.rb", "y.txt"]
false
true

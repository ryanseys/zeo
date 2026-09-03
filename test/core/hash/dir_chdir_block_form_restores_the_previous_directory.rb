# `Dir.chdir`'s block form restores the previous directory afterwards.

dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_chdir_#{Process.pid}")
Dir.mkdir(dir)
begin
  before = Dir.pwd
  Dir.chdir(dir) do
    p Dir.pwd != before
  end
  p Dir.pwd == before
ensure
  Dir.rmdir(dir)
end
__END__
true
true

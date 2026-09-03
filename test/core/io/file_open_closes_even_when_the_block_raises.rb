# `File.open`'s block form closes the file even when the block RAISES --
# that ensure is the whole reason the idiom exists.

dir = File.join(ENV.fetch("TMPDIR", "/tmp"), "zeo_e2e_raise_#{Process.pid}")
Dir.mkdir(dir)
begin
  path = File.join(dir, "x.txt")
  File.write(path, "data")
  handle = nil
  begin
    File.open(path, "r") do |f|
      handle = f
      raise "boom"
    end
  rescue RuntimeError => e
    puts "rescued: #{e.message}"
  end
  p handle.closed?
  File.delete(path)
ensure
  Dir.rmdir(dir)
end
__END__
rescued: boom
true

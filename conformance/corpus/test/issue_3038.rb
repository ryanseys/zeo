pth = "sp_issue_3038.tmp"
File.write(pth, "hi\n")
File.open(pth) do |f|
  p f.readbyte
  f.ungetbyte(104)
  p f.readbyte
  p f.binmode?
  p f.to_io.equal?(f)
  p f.close_on_exec?
  p f.pread(2, 0)
  p f.advise(:normal)
  p((f.autoclose = true))
  p((f.close_write rescue $!.class))
  p((f.close_read rescue $!.class))
end
File.open(pth, "r+") { |f| p f.pwrite("X", 0) }
p File.read(pth)
File.open(pth) { |f| cps = []; f.each_codepoint { |cp| cps << cp }; p cps }
File.delete(pth)

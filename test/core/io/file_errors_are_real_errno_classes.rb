# A missing path raises the right `Errno::*`, with CRuby's message shape --
# and it is catchable by its PARENT (`SystemCallError`), which is what the
# prelude hierarchy buys.

begin
  File.read("/definitely/not/here")
rescue Errno::ENOENT => e
  puts "#{e.class}: #{e.message}"
end
p Errno::ENOENT.superclass
p Errno::ENOENT.ancestors.include?(StandardError)
begin
  Dir.entries("/definitely/not/here")
rescue SystemCallError => e
  puts e.class
end
begin
  File.read(5)
rescue TypeError => e
  puts "TypeError"
end
__END__
Errno::ENOENT: No such file or directory @ rb_sysopen - /definitely/not/here
SystemCallError
true
Errno::ENOENT
TypeError

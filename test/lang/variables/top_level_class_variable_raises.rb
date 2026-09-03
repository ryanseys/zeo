# A bare `@@x` outside any class/module body has no owning cref, and ruby
# 4.0.6 raises `RuntimeError: class variable access from toplevel` for both
# the write and the read (it was only a warning in older rubies). zeo used
# to store it on `Object` and read it back; this pins the corrected
# behaviour, message included.

begin
  @@plain = 42
rescue RuntimeError => e
  puts e.message
end
begin
  puts @@plain
rescue RuntimeError => e
  puts e.message
end
__END__
class variable access from toplevel
class variable access from toplevel

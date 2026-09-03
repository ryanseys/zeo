p self.class
begin
  self.class.const_get(:MissingXYZ)
rescue => e
  p e.class
  puts e.message
end
def topm; self.class; end
p topm
__END__
Object
NameError
uninitialized constant MissingXYZ
Object

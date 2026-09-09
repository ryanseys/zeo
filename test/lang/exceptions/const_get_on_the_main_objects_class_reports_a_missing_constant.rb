# The message and class of a NameError raised through self.class.const_get at the top level.
# (spinel issue #3035)
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

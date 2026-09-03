p ObjectSpace.count_objects.class
p ObjectSpace.garbage_collect
begin
  ObjectSpace.each_object(String) {}
rescue NotImplementedError => e
  puts "each_object: #{e.message}"
end
begin
  ObjectSpace._id2ref(8)
rescue NotImplementedError => e
  puts "_id2ref: #{e.message}"
end
# GC surface stays no-op-but-shaped
p GC.start
p GC.count.class
__END__
Hash
nil
each_object: ObjectSpace.each_object over instances needs the allocation registry, which only ZEO_GC=1 arms
_id2ref: ObjectSpace._id2ref is not available (zeo has no id-to-object table)
nil
Integer

# `attr_accessor`/`attr_reader`/`attr_writer` validate the name ("invalid
# attribute name '1bad'"); zeo accepts anything and mints :"1bad" rows,
# which then leak into instance_methods. (Found by the 2026-08-24 probe
# sweep.)
c = Class.new
begin
  c.send(:attr_accessor, "1bad")
rescue NameError => e
  puts e.message
end
p c.instance_methods(false)
__END__
invalid attribute name '1bad'
[]

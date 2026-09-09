# Each names its own kind, and the address is a real pointer padded the way
# every other default inspect pads one.
m = Module.new
puts m.to_s
puts m.inspect
puts Class.new.inspect
__END__
#<Module:0xADDR>
#<Module:0xADDR>
#<Class:0xADDR>

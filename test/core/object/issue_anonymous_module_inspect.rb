# Two compounding divergences in the default inspect of an anonymous
# Class/Module: (1) an anonymous Module renders as `#<Class:0x...>` instead
# of `#<Module:0x...>` -- zeo doesn't distinguish the two; (2) the address
# zeo renders is a short sequential counter (`0x40000000`) rather than a
# real pointer padded to 16 hex digits like every other object's default
# inspect, so it never matches the golden's `0xADDR` address-normalization
# even once the Module/Class mislabeling above is fixed.
m = Module.new
puts m.to_s
puts m.inspect
puts Class.new.inspect
__END__
#<Module:0xADDR>
#<Module:0xADDR>
#<Class:0xADDR>

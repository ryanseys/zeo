# A numeric method with a String argument raises the ArgumentError ruby
# raises, not a TypeError.
#
r001 = (5.div("x") rescue $!.class)
p r001

r002 = (5.gcdlcm("x") rescue $!.class); p r002
r003 = (5.modulo("x") rescue $!.class); p r003
r004 = (5.remainder("x") rescue $!.class); p r004
r005 = (5.divmod("x") rescue $!.class); p r005
r006 = (5.pow("x") rescue $!.class); p r006
r007 = (5.fdiv("x") rescue $!.class); p r007
r008 = (5.digits("x") rescue $!.class); p r008
r009 = (5.to_s("x") rescue $!.class); p r009
r010 = (5.ceildiv("x") rescue $!.class); p r010
r011 = (Integer.sqrt("x") rescue $!.class); p r011
r012 = (1.5.fdiv("x") rescue $!.class); p r012
r013 = (1.5.divmod("x") rescue $!.class); p r013
r014 = (1.5.round("x") rescue $!.class); p r014
r015 = (1.5.floor("x") rescue $!.class); p r015
r016 = (1.5.ceil("x") rescue $!.class); p r016
r017 = (1.5.truncate("x") rescue $!.class); p r017

r018 = (5.coerce("x") rescue $!.class); p r018
r019 = (1.5.coerce("x") rescue $!.class); p r019
r020 = (5.clamp("a", "z") rescue $!.class); p r020
r021 = (1.upto("a").to_a rescue $!.class); p r021
r022 = (3.downto("a").to_a rescue $!.class); p r022
r023 = (1.step("z", 1).to_a rescue $!.class); p r023
r024 = (1.5.between?("a", "z") rescue $!.class); p r024

r025 = (5.between?("a", "z") rescue $!.class); p r025   # Ruby: ArgumentError   Spinel: false

p((5.gcd("x") rescue $!.class))     # both: TypeError
p((5.lcm("x") rescue $!.class))     # both: TypeError
p((1.5 + "x" rescue $!.class))      # both: TypeError
p((1.5 ** "x" rescue $!.class))     # both: TypeError
__END__
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
TypeError
ArgumentError
ArgumentError
ArgumentError
ArgumentError
ArgumentError
ArgumentError
ArgumentError
ArgumentError
TypeError
TypeError
TypeError
TypeError

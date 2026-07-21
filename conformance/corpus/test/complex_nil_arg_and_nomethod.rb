# Kernel#Complex rejects a nil component (TypeError), and an undefined method
# on a Complex/Rational value raises NoMethodError instead of aborting.
begin; Complex(nil); rescue => e; p e.class; end
begin; Complex(1, nil); rescue => e; p e.class; end
begin; Complex(2, 3).floor; rescue NoMethodError => e; puts e.message; end
begin; Rational(1, 2).each; rescue => e; p e.class; end
p Complex(2, 3).real
p Complex(2, 3).abs2

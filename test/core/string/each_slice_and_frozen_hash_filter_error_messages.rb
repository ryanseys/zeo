# each_slice(0) says "invalid slice size", each_cons(0) just "invalid
# size"; an in-place Hash filter on a frozen receiver raises FrozenError.

begin; [1, 2, 3].each_slice(0).to_a; rescue ArgumentError => e; puts e.message; end
begin; [1, 2, 3].each_cons(0).to_a; rescue ArgumentError => e; puts e.message; end
begin; {a: 1}.freeze.reject! { |k, v| true }; rescue => e; puts e.class; end
__END__
invalid slice size
invalid size
FrozenError

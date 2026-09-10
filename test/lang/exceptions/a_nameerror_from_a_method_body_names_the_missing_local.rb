# NameError#name is the identifier that did not resolve, raised inside a def and at the top level.
def undef_ref_m005; some_undefined_local_x005; end
n = begin; undef_ref_m005; rescue => e; e.name; end
p n
n2 = begin; some_other_undef; rescue NameError => e; e.name; end
p n2
__END__
:some_undefined_local_x005
:some_other_undef

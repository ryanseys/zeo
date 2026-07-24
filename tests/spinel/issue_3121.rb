def undef_ref_m005; some_undefined_local_x005; end
n = begin; undef_ref_m005; rescue => e; e.name; end
p n
n2 = begin; some_other_undef; rescue NameError => e; e.name; end
p n2

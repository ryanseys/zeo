# ENV#assoc, #key, #slice and #values_at each raise TypeError for a Symbol, and answer for a String.
ENV['ZZ_A'] = 'zzv'
def cls; begin; yield; rescue => e; e.class; end; end
p(cls { ENV.assoc(:sym) })
p(cls { ENV.key(:sym) })
p(cls { ENV.slice(:sym) })
p(cls { ENV.values_at(:sym) })
p ENV.assoc('ZZ_A')
p ENV.key('zzv')
p ENV.slice('ZZ_A')
p ENV.values_at('ZZ_A')
__END__
TypeError
TypeError
TypeError
TypeError
["ZZ_A", "zzv"]
"ZZ_A"
{"ZZ_A" => "zzv"}
["zzv"]

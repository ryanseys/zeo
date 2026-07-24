# ENV's enumeration surface rides a StrStr-hash snapshot (__env_to_h), so the
# Hash machinery serves keys/each/select/count{}/fetch{}/inspect (#2742, #2744,
# #2745); direct arms cover identity, copy refusal, mutators, arity and
# key-type validation (#2743, #2746, #2747, #2765, #2766, #2773).
ENV['ZZ_A'] = '1'
p ENV.keys.include?('ZZ_A')
p ENV.count { |k, v| k == 'ZZ_A' }
p ENV.fetch('ZZ_NOPE') { |k| "missing:#{k}" }
p ENV.to_s
r = (ENV.dup rescue $!.message); p r
r = (ENV.clone rescue $!.message); p r
r = (ENV.freeze rescue $!.message); p r
r = (ENV.fetch rescue $!.message); p r
r = (ENV.fetch('A', 'd', 'x') rescue $!.message); p r
r = (ENV['a', 'b'] rescue $!.message); p r
r = (ENV[:sym] rescue $!.class); p r
r = (ENV[5] rescue $!.class); p r
p(ENV['ZZ_A'] = '9')
v = (ENV['ZZ_A'] = '7')
p v
p ENV['ZZ_A']
p ENV.store('ZZ_B', '3')
p ENV.delete('ZZ_B')
p ENV.delete('ZZ_NOPE')
p ENV.inspect.class
p ENV.hash.class

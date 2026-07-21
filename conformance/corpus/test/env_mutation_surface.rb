# ENV follow-ups (#2831, #2832): predicate arity, the wider Enumerable
# surface, and the mutators (update/merge!/clear/shift/delete_if family).
ENV['ZZ_A'] = '1'
r1 = (ENV.has_key? rescue $!.class);             p r1
r2 = (ENV.key?('ZZ_A', 'ZZ_B') rescue $!.class); p r2
r3 = (ENV.include?('a', 'b') rescue $!.class);   p r3
r4 = (ENV.member?('a', 'b') rescue $!.class);    p r4
p ENV.key?('ZZ_A')
ENV['ZZ_A'] = '1'
ENV['ZZ_B'] = '2'
p ENV.value?('1')
p ENV.to_hash.class
p ENV.first.class
p ENV.update('ZZ_A' => '9')['ZZ_A']
p ENV['ZZ_A']
p ENV.merge!('ZZ_C' => '3')['ZZ_C']
p ENV.frozen?
p ENV.has_value?('3')
ENV.delete_if { |k, v| k.start_with?('ZZ_') }
p ENV['ZZ_A']
p ENV.key?('ZZ_C')
ENV['ZK_1'] = 'a'
ENV['ZK_2'] = 'b'
ENV.keep_if { |k, v| !k.start_with?('ZK_') }
p ENV.key?('ZK_1')
p ENV.tally.class
p ENV.take(1).class
p ENV.min.class
sl_n = 0
ENV.each_slice(2) { |sl| sl_n += sl.length }
p sl_n >= 2
p ENV.grep(/^nope$/)
sh = ENV.shift
p sh.class
p ENV.entries.class

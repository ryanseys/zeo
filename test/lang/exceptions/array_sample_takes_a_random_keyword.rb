# Array#sample accepts `random:` and draws from that generator.
r = ([1, 2, 3].sample(random: Random.new(1)) rescue $!.class)
p(r.nil? || [1, 2, 3].include?(r))
__END__
true

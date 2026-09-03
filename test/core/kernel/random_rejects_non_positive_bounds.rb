begin; Random.new(1).rand(0); rescue ArgumentError => e; puts e.message; end
begin; Random.new(1).rand(-3); rescue ArgumentError => e; puts e.message; end
__END__
invalid argument - 0
invalid argument - -3

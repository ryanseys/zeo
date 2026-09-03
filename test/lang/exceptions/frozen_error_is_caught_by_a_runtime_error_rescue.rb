g = [1].freeze
begin
  g[0] = 2
rescue RuntimeError => e
  puts "runtime-rescued"
end
__END__
runtime-rescued

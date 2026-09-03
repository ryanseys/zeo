def base
  40
end
f = ->(x) { base + x }
puts f.call(2)
__END__
42

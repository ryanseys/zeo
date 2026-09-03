begin
  p Regexp.new("a", nil, "n").encoding.name
rescue ArgumentError, TypeError => e
  p e.class
end
__END__
ArgumentError

begin
  42.coerce("l")
rescue ArgumentError => e
  p e.class
end
begin
  2.5.coerce("")
rescue ArgumentError => e
  p e.class
end
__END__
ArgumentError
ArgumentError

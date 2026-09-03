begin
  "\xff".force_encoding("UTF-8") =~ /a/
  p :no_raise
rescue ArgumentError => e
  p e.class
end
__END__
ArgumentError

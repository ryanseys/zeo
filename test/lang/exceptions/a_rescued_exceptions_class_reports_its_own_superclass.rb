# e.class.superclass names the parent of the class actually raised, for several built-in exceptions.
# (spinel issue #3031)
begin
  raise ArgumentError, "x"
rescue => e
  p e.class.superclass
end
begin
  raise TypeError, "y"
rescue => e
  p e.class.superclass
end
begin
  {}.fetch(:missing)
rescue KeyError => e
  p e.class.superclass
end
p ArgumentError.superclass
__END__
StandardError
StandardError
IndexError
StandardError

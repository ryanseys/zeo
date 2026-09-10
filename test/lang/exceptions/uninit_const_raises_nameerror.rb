# Uninitialised constant reads raise NameError instead of
# silently returning 0.

begin
  puts NONEXISTENT_CONST
rescue NameError
  puts "uninit raised"
end

# A self-referential initialiser `X = X + 1` reads X before X is bound, so it
# raises NameError rather than reading X as zero and assigning 1.
begin
  X = X + 1
  puts X
rescue NameError
  puts "self-ref raised"
end
__END__
uninit raised
self-ref raised

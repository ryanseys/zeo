# An unknown character property raises RegexpError with ruby's one-line
# message ("invalid character property name {Nope}: /\p{Nope}/"); zeo
# leaks the rust regex crate's multi-line rendering. The unclosed-paren
# and bad-quantifier messages already match. (Found by the 2026-08-24
# probe sweep.)
begin
  Regexp.new("\\p{Nope}")
rescue RegexpError => e
  puts e.message
end
__END__
invalid character property name {Nope}: /\p{Nope}/

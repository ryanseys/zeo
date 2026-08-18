begin
  [1, 2, 3].chunk_while
rescue ArgumentError => e
  p e.class
end
begin
  [1, 2, 3].slice_when
rescue ArgumentError => e
  p e.class
end

begin
  "s".no_such
rescue NoMethodError => e
  p e.receiver
end
begin
  42.frobnicate
rescue NoMethodError => e
  p e.receiver
end
begin
  3.14.no_way
rescue NoMethodError => e
  p e.receiver
end
__END__
"s"
42
3.14

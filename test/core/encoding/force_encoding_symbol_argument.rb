begin
  (+"x").force_encoding(:a)
rescue TypeError => e
  p e.class
end
__END__
TypeError

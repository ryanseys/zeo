begin
  catch("str") { throw "str", :s }
rescue UncaughtThrowError => e
  p e.class
end
__END__
UncaughtThrowError

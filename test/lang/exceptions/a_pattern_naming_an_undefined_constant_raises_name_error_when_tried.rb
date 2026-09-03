x = 5
r = begin
  case x
  in NopeClass then "a"
  else "b"
  end
rescue => e
  "#{e.class}: #{e.message}"
end
puts r
__END__
NameError: uninitialized constant NopeClass

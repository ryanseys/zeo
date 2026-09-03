h = { a: 1, b: 2, c: 3 }
case h
in { a: Integer => av, **rest }
  puts av
  puts rest.length
end
__END__
1
2

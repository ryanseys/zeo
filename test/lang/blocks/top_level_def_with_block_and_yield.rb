def twice
  yield 1
  yield 2
end
twice { |i| puts "got #{i}" }
__END__
got 1
got 2

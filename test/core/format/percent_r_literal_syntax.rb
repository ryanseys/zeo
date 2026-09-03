r = %r{foo/bar}
puts r.match?("xxfoo/barxx")
puts r.source
__END__
true
foo/bar

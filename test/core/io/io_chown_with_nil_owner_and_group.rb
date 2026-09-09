# It answers 0, changing nothing.
# (spinel issue #3104)
path = "/tmp/spinel_issue_3104.txt"
File.write(path, "x")
File.open(path) { |f| p f.chown(nil, nil) }
File.delete(path)
__END__
0

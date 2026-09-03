# A program prism cannot parse is rejected before anything runs, with the
# parser's own diagnostic.
def broken(
  puts 1
__END__
#@ stderr
zeo::parse

  × parse error: unexpected integer; expected a `)` to close the parameters
  help: a Ruby syntax error, not a zeo gap

#@ exit 1

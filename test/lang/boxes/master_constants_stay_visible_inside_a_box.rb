# The other half of that line: a box IS a copy of master, so the constants
# the runtime installed before main ran stay visible inside it -- the bare
# read's fallback tail reaches those and stops, rather than continuing into
# main's own `Object` table.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

VERSION_COPY = RUBY_VERSION
box = Ruby::Box.new
box.eval("puts RUBY_VERSION")
box.eval("begin; p VERSION_COPY; rescue NameError; puts 'invisible'; end")
__END__
4.0.6
invisible

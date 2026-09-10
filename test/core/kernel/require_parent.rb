# Regression: `require_relative ".."` must load the parent directory's
# `<dir>.rb`. Ruby normalizes "views/articles/.." -> "views" and appends
# ".rb" -> "views.rb", so ".rb" must not be glued onto a literal ".." to
# form a bogus "...rb". The idiom is a directory of views each pulling in
# their aggregator.
require_relative "require_parent/views/articles/index"
__END__
views aggregator loaded
index loaded

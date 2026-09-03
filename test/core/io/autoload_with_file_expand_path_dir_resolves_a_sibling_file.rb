# The dominant stdlib/bundler idiom: `autoload :C, File.expand_path("c",
# __dir__)` -- a computed sibling path. Resolved at compile time from the
# requiring file's directory.

require_relative "autoload_with_file_expand_path_dir_resolves_a_sibling_file/main"
__END__
mirror!

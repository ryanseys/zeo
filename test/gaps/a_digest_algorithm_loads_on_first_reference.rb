# `require "digest"` defines `Digest` alone: each algorithm class loads on
# its first reference (`Digest.const_missing`), so `defined?` answers nil
# for one until something names it or requires its file.
require "digest"
p defined?(Digest::SHA256)
p Digest::SHA256.name
p defined?(Digest::SHA256)
__END__
nil
"Digest::SHA256"
"constant"

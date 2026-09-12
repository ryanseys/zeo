# `Digest.bubblebabble` before `require "digest/bubblebabble"`.
require "digest"
p Digest.singleton_methods(false).sort
p Digest.respond_to?(:bubblebabble)
require "digest/bubblebabble"
p Digest.singleton_methods(false).sort
p Digest.bubblebabble("abc")
__END__
[:const_missing, :hexencode]
false
[:bubblebabble, :const_missing, :hexencode]
"ximek-domex"

# A box loading a FILE (rather than an eval'd string) isolates the same
# way. The two routes reach the compiler differently -- a literal
# `box.eval` is spliced at compile time and a `require_relative` brings in
# a whole unit -- so both need saying.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

require_relative "a_boxs_required_file_patches_a_shared_class_privately/main"
__END__
false
"go"

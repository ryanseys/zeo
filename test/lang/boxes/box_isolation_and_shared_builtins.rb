# The keystone: a box-required file's classes are DISTINCT from main's
# (same file, different class objects), builtins are SHARED
# (`box::String == String`), a box top-level constant is readable
# externally, and box gvars are invisible in main -- all per the CRuby
# box model.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

require_relative "box_isolation_and_shared_builtins/main"
__END__
"main widget"
"box widget"
false
true
99
nil
#<Ruby::Box:4,user,optional>

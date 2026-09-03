# Exceptions cross the boundary as plain references: a box-defined
# `BoxError < StandardError` raised from box code is rescuable in main
# through the SHARED bootstrap superclass chain, and `e.class` names it.
#@ ruby: -W:no-experimental
#@ env: RUBY_BOX=1

require_relative "box_exceptions_are_rescuable_in_main/main"
__END__
rescued: from the box (BoxError)

# Real Ruby gives every file its own top-level local scope: main's
# `count` and the lib's `count` (mutated through a block, exercising
# the rename pass inside shared-scope block bodies) never touch. The
# lib hands its result out through a global -- the only channel real
# Ruby shares.

require_relative "a_required_files_top_level_locals_are_isolated_from_the_main_file/main"
__END__
5
103
2

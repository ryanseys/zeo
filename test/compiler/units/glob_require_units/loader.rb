# The loader sits INSIDE the tree it globs, exactly as tzinfo's ts_all.rb
# does -- so the demanded unit walk is bounded to this directory.
Dir[File.join(__dir__, "part_*.rb")].sort.each { |t| require t }

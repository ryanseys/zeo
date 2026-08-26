# `$LOADED_FEATURES` names the FILE a require loaded, never the spelling the
# require asked under. CRuby lists absolute paths, and real libraries read that
# list: rubygems' `already_loaded?` compares `"#{load_path_entry}/#{file}"`
# against it, and bundler's `shared_helpers` asks each entry
# `start_with?(resolved_path)`.
#
# A compiled-in unit used to append the require's own spelling instead, so a
# short name landed where a path belonged and both those checks answered wrong.
# Each unit row now carries its own source file, and that is what the load
# records.

require_relative "a_unit_records_its_own_path_in_loaded_features/part" if ENV["NEVER"]

def load_part
  require_relative "a_unit_records_its_own_path_in_loaded_features/part"
end

load_part
entry = $LOADED_FEATURES.last
p entry.start_with?("/")
p File.basename(entry)
p File.exist?(entry)
# One entry per FILE, however many spellings reach it.
p $LOADED_FEATURES.count { |f| File.basename(f) == "part.rb" }

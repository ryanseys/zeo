require "set"
p $LOADED_FEATURES.any? { |f| f.include?("set") }
p $LOADED_FEATURES.empty?

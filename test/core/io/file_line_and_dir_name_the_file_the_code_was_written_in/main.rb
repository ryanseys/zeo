require_relative "helper"
puts File.basename(helper_file)
puts helper_line
puts File.basename(__FILE__)
puts __LINE__
puts helper_dir == __dir__
puts __dir__.start_with?("/")

# A module_function-promoted method whose body calls a string method
# on its param, invoked with a string-literal arg, must route the
# param to the string path (not the int "0" NoMethodError fallback).
# Mirrors strings-ansi's Strings::ANSI.sanitize gsub-with-regex-const.
module Strings
  module ANSI
    ANSI_MATCHER = /\e\[[0-9;]*m/
    def sanitize(string)
      string.gsub(ANSI_MATCHER, "")
    end
    module_function :sanitize
  end
end
puts Strings::ANSI.sanitize("\e[31mRED\e[0m and \e[1mbold\e[0m text")

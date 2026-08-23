puts "lib.rb ran"
module Outer
  # Spelled with the extension, which is a different STRING from the one a
  # `require_relative` of the same file resolves to -- and the same file.
  autoload :Target, "#{__dir__}/target.rb"
end

module MyLib
  # `ActiveSupport::Autoload`'s shape, in miniature: an `autoload` written in
  # plain Ruby that derives the path from the constant and hands it to the real
  # `Module#autoload` through `super`. Nothing here is known to the compiler --
  # the string only exists while the program runs.
  module Autoload
    def autoload(const_name, path = nil)
      path ||= File.expand_path("mylib/#{const_name.to_s.downcase}", __dir__)
      super const_name, path
    end
  end

  extend Autoload

  autoload :Model
end

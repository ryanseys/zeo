# Loaded only when `Debugger.attach` runs. Everything below is invisible until
# then -- which is the whole point of the golden beside this file.
module Debugger
  ANNOUNCED = []

  class ::Module
    undef method_added
    def method_added(mid)
      Debugger::ANNOUNCED << mid
    end
  end
end

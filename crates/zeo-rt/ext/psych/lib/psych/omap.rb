# frozen_string_literal: true
module Psych
  # `!!omap` loads as one of these, which is why an ordered map answers
  # like the Hash it already is.
  class Omap < ::Hash
  end
end

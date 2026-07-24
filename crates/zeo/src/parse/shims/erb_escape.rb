# frozen_string_literal: true
#
# zeo's stand-in for CRuby's C `erb/escape` extension (`erb/escape.so`). It
# defines `ERB::Escape#html_escape` over the native `CGI.escapeHTML`, exactly
# what erb/util.rb's `rescue LoadError` fallback defines when the C extension
# isn't built. Splicing it makes `require "erb/escape"` SUCCEED (rather than
# raising LoadError and taking the fallback), so `module ERB::Util; include
# ERB::Escape` resolves -- matching CRuby with the extension present.
module ERB::Escape
  def html_escape(s)
    CGI.escapeHTML(s.to_s)
  end
  module_function :html_escape
end

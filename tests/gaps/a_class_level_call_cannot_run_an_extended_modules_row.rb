# `CGI` EXTENDS `CGI::Escape`, so in ruby every class-level escape call runs the
# module's own instance method and reports it: `CGI.method(:escapeHTML).owner`
# is `CGI::EscapeExt`, `CGI.method(:escapeElement).owner` is `CGI::Escape`.
#
# zeo carries the extend edge (`zeo_abi::BUILTIN_EXTENDS`), so both ancestor
# chains are right and so is everything the INCLUDE side reaches. The extend
# side is not: a module's instance row takes an `RObj`, and a class-level call
# arrives with a `RubyValue::Class`, which is not one -- so `CGI` keeps its own
# class rows over the same bodies and reports itself as their owner.
#
# Every other observable agrees. Closing this needs a class-level call to be
# able to run a module's instance row, which is a dispatch change rather than a
# CGI one; the same shape would then serve any builtin that extends a module.
require "cgi/escape"

p CGI.method(:escapeHTML).owner
p CGI.method(:escapeElement).owner
p CGI.method(:h).owner
p CGI.singleton_class.instance_method(:escape).owner

# What already agrees, kept beside it so a fix cannot trade one for the other.
p CGI.singleton_class.ancestors.take(3)
p CGI.instance_method(:escapeHTML).owner
p CGI.escapeHTML("<a>")

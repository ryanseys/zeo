# #572 follow-up to #573 (Sam Ruby). After 8ca93b4 wired late-
# reopened module methods into namespaced including classes, the
# struct-layout side missed: an ivar written by a late-reopened
# method (`@session ||= ...` in `Dispatch#post`, then included by
# `NS::IntegrationTest`) wasn't added to the host class's
# @cls_ivar_names. The C emit referenced `self->iv_session` against
# a struct that didn't declare the field; C compile failed.
#
# Fix: reconcile_class_includes re-runs collect_ivars on any class
# whose method count grew, so the now-attached method bodies get
# scanned for InstanceVariable*Write nodes.

module Dispatch
end

class Base
end

module NS
  class IntegrationTest < Base
    include Dispatch
  end
end

module Dispatch
  def post(path)
    @session ||= "session-obj"
    puts @session
    puts path
  end
end

NS::IntegrationTest.new.post("/foo")
